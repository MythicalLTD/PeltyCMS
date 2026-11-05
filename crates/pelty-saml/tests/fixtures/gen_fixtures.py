# Generates interop fixtures with signxml (lxml/libxml2 canonicalization),
# an implementation independent of pelty-saml. Run once; outputs are committed.
import subprocess, sys
from lxml import etree
from signxml import XMLSigner, methods, SignatureConstructionMethod
from signxml.algorithms import CanonicalizationMethod

def keypair(kind):
    if kind == "rsa":
        subprocess.run(["openssl","req","-x509","-newkey","rsa:2048","-nodes","-keyout",f"{kind}.key","-out",f"{kind}.crt","-days","3650","-subj","/CN=fixture-idp"],check=True,capture_output=True)
    else:
        subprocess.run(["openssl","req","-x509","-newkey","ec","-pkeyopt","ec_paramgen_curve:P-256","-nodes","-keyout",f"{kind}.key","-out",f"{kind}.crt","-days","3650","-subj","/CN=fixture-idp"],check=True,capture_output=True)
    return open(f"{kind}.key","rb").read(), open(f"{kind}.crt","rb").read()

T0="2026-10-30T10:00:00Z"; T1="2026-10-30T10:05:00Z"
XML=f"""<samlp:Response xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol" ID="_resp1" Version="2.0" IssueInstant="{T0}" Destination="https://billing.example/acs" InResponseTo="_req42">
  <saml:Issuer xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion">https://idp.fixture/entity</saml:Issuer>
  <samlp:Status><samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:Success"/></samlp:Status>
  <saml:Assertion xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion" xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" ID="_assert1" Version="2.0" IssueInstant="{T0}">
    <saml:Issuer>https://idp.fixture/entity</saml:Issuer>
    <saml:Subject>
      <saml:NameID Format="urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress">ion@example.com</saml:NameID>
      <saml:SubjectConfirmation Method="urn:oasis:names:tc:SAML:2.0:cm:bearer">
        <saml:SubjectConfirmationData NotOnOrAfter="{T1}" Recipient="https://billing.example/acs" InResponseTo="_req42"/>
      </saml:SubjectConfirmation>
    </saml:Subject>
    <saml:Conditions NotBefore="{T0}" NotOnOrAfter="{T1}">
      <saml:AudienceRestriction><saml:Audience>https://billing.example/sp</saml:Audience></saml:AudienceRestriction>
    </saml:Conditions>
    <saml:AuthnStatement AuthnInstant="{T0}" SessionIndex="s1"><saml:AuthnContext><saml:AuthnContextClassRef>urn:oasis:names:tc:SAML:2.0:ac:classes:Password</saml:AuthnContextClassRef></saml:AuthnContext></saml:AuthnStatement>
    <saml:AttributeStatement>
      <saml:Attribute Name="mail" FriendlyName="email"><saml:AttributeValue xsi:type="xs:string">ion@example.com</saml:AttributeValue></saml:Attribute>
      <saml:Attribute Name="memberOf"><saml:AttributeValue xsi:type="xs:string">Clients &amp; Co</saml:AttributeValue><saml:AttributeValue xsi:type="xs:string">vip</saml:AttributeValue></saml:Attribute>
    </saml:AttributeStatement>
  </saml:Assertion>
</samlp:Response>"""

for kind, alg in [("rsa","rsa-sha256"),("ec","ecdsa-sha256")]:
    key, crt = keypair(kind)
    root = etree.fromstring(XML.encode())
    ns={"saml":"urn:oasis:names:tc:SAML:2.0:assertion"}
    a = root.find("saml:Assertion", ns)
    signer = XMLSigner(method=methods.enveloped, signature_algorithm=alg, digest_algorithm="sha256", c14n_algorithm=CanonicalizationMethod.EXCLUSIVE_XML_CANONICALIZATION_1_0)
    signed = signer.sign(a, key=key, cert=crt, reference_uri="_assert1", always_add_key_value=False)
    # signxml puts Signature as last child; SAML schema wants it after Issuer.
    sig = signed.find("{http://www.w3.org/2000/09/xmldsig#}Signature")
    signed.remove(sig); signed.insert(1, sig)
    root.replace(a, signed)
    open(f"response_{kind}.xml","wb").write(etree.tostring(root))
    print(kind, "ok")
