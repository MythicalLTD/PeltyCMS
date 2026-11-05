# Regenerates whmcs.sql: python3 crates/pelty-import/tests/fixtures/whmcs_fixture.py
import hashlib, base64
CC = "Xk2v9QpL7mZr4tYw1sN8cB3dF6gH0jKa"  # fixture cc_encryption_hash
def md5h(b): return hashlib.md5(b).hexdigest()
def enc(cc, plain, iv):
    m = md5h(cc.encode()); key = (md5h(m.encode()) + m).encode()
    hk = hashlib.sha1(key).digest()
    out = bytes(a ^ b for a, b in zip(iv, hk))
    k = iv; p = plain.encode(); body = bytearray()
    for c, x in enumerate(p):
        if c and c % 20 == 0:
            k = hashlib.sha1(k + p[c-20:c]).digest()
        body.append(k[c % 20] ^ x)
    return base64.b64encode(out + bytes(body)).decode()
SRV_PW = enc(CC, "R00t-Pa$$-for-whm-server-01!", bytes(range(1, 21)))
BOB_SALT = "a1#Zx"
BOB_MD5 = md5h((BOB_SALT + "Bob-Old-Pass").encode()) + ":" + BOB_SALT
ANA_BCRYPT = "$2y$10$9b2PCEp5BOppVq8RTzpBvuOmSPQ3gmS68ujZpY4jniH9lqge5a1im"  # Ana-Pass-2024

T = {}
def table(name, cols, rows): T[name] = (cols, rows)

table("tblconfiguration", [("setting","varchar(64)"),("value","text")], [("CompanyName","Fixture Hosting @T@"),("Version","8.10.1-release.1")])
table("tblcurrencies", [("id","int(10)"),("code","text"),("prefix","text"),("suffix","text"),("format","int(1)"),("rate","decimal(10,5)"),("default","int(1)")],
      [(1,"USD","$"," USD",1,"1.00000",1),(2,"EUR","€"," EUR",3,"0.92000",0)])
table("tblclientgroups", [("id","int(10)"),("groupname","varchar(45)"),("groupcolour","varchar(45)"),("discountpercent","decimal(10,2)"),("susptermexempt","text"),("separateinvoices","text")],
      [(1,"Resellers @T@","#ff9900","15.00","on","")])
table("tblclients", [("id","int(10)"),("uuid","varchar(255)"),("firstname","text"),("lastname","text"),("companyname","text"),("email","text"),("address1","text"),("address2","text"),("city","text"),("state","text"),("postcode","text"),("country","text"),("phonenumber","text"),("tax_id","varchar(128)"),("password","text"),("authmodule","text"),("currency","int(10)"),("defaultgateway","text"),("credit","decimal(16,2)"),("taxexempt","tinyint(1)"),("groupid","int(10)"),("cardtype","varchar(255)"),("cardlastfour","text"),("cardnum","blob"),("startdate","blob"),("expdate","blob"),("issuenumber","blob"),("bankname","text"),("banktype","text"),("bankcode","blob"),("bankacct","blob"),("datecreated","date"),("notes","text"),("status","enum('Active','Inactive','Closed')"),("language","text"),("created_at","timestamp"),("updated_at","timestamp")],
  [(1,"7d3c0f6e-1111-4b8e-9b55-000000000001","Ana","Popescu","","ana-@T@@example.com","Str. Lalelelor 12","Ap. 4","Cluj-Napoca","Cluj","400001","RO","+40.744123456","RO12345678","","",1,"stripe","25.00",0,0,"Visa","4242","0x9F2C11AB","","0xAB01","","","","","","2023-05-14","Prefers email","Active","romanian","2023-05-14 10:00:00","2024-02-01 09:00:00"),
   (2,"7d3c0f6e-1111-4b8e-9b55-000000000002","Bob","O'Brien","O'Brien \"Web\" Ltd","bob-@T@@example.org","1 High St","","Dublin","","D02","IE","+353.1234567","",BOB_MD5,"",2,"paypal","0.00",0,1,"","",None,None,None,None,None,None,None,None,"2015-03-02","Legacy client\nsince 2015","Active","","2015-03-02 00:00:00","2015-03-02 00:00:00"),
   (3,"7d3c0f6e-1111-4b8e-9b55-000000000003","Carla","Weiß","Weiß GmbH","carla-@T@@example.de","Hauptstr. 5","","Wien","","1010","AT","","ATU12345678","sha256-unknown-format","",1,"banktransfer","0.00",0,0,"MasterCard","5555","0x00FF","","","","Erste","Checking","0x0102","0x0304","2020-01-01","","Inactive","german","2020-01-01 00:00:00","2020-01-01 00:00:00")])
table("tblusers", [("id","int(10)"),("first_name","varchar(255)"),("last_name","varchar(255)"),("email","varchar(255)"),("password","varchar(255)"),("second_factor","varchar(255)"),("email_verified_at","timestamp"),("created_at","timestamp")],
      [(1,"Ana","Popescu","ANA-@T@@example.com",ANA_BCRYPT,"",None,"2023-05-14 10:00:00")])
table("tblcreditcards", [("id","int(10)"),("pay_method_id","int(10)"),("card_type","varchar(255)"),("last_four","varchar(255)"),("expiry_date","timestamp"),("card_data","blob")],
      [(1,1,"Visa","4242","2027-12-01 00:00:00","0xDEADBEEF4111111111111111")])
table("tblcontacts", [("id","int(10)"),("userid","int(10)"),("firstname","text"),("lastname","text"),("companyname","text"),("email","text"),("address1","text"),("city","text"),("country","text"),("phonenumber","text"),("password","text"),("generalemails","int(1)"),("invoiceemails","int(1)"),("supportemails","int(1)")],
      [(1,1,"Mihai","Popescu","","billing-@T@@example.com","Str. Lalelelor 12","Cluj-Napoca","RO","+40.744000111","",0,1,0),
       (2,2,"Tech","Contact","","tech-@T@@example.org","1 High St","Dublin","IE","","",1,0,1)])
table("tblproductgroups", [("id","int(10)"),("name","text"),("slug","varchar(128)"),("headline","text"),("tagline","text"),("orderfrmtpl","text"),("disabledgateways","text"),("hidden","tinyint(1)"),("order","int(1)")],
      [(1,"Shared Hosting @T@","shared-@T@","Fast & simple","","","",0,1),(2,"VPS @T@","vps-@T@","","","","",1,2)])
table("tblproducts", [("id","int(10)"),("type","text"),("gid","int(10)"),("name","text"),("slug","varchar(128)"),("description","text"),("hidden","tinyint(1)"),("showdomainoptions","tinyint(1)"),("paytype","text"),("servertype","text"),("servergroup","int(10)"),("configoption1","text"),("configoption2","text"),("retired","tinyint(1)"),("order","int(10)")],
      [(1,"hostingaccount",1,"Starter @T@","starter-@T@","<p>1 site, 10 GB SSD</p>",0,1,"recurring","cpanel",1,"starter_pkg","",0,1),
       (2,"hostingaccount",1,"Pro @T@","pro-@T@","Unlimited sites",0,1,"recurring","cpanel",1,"pro_pkg","",0,2),
       (3,"server",2,"VPS-2G @T@","vps2g-@T@","2 GB RAM VPS",0,0,"recurring","virtualizor",2,"","",0,1),
       (4,"other",1,"Old Plan @T@","old-@T@","",1,0,"onetime","",0,"","",1,9)])
P = ["id","type","currency","relid","msetupfee","qsetupfee","ssetupfee","asetupfee","bsetupfee","tsetupfee","monthly","quarterly","semiannually","annually","biennially","triennially"]
table("tblpricing", [(c, "int(10)" if c in ("id","currency","relid") else ("enum('product','addon','configoptions','domainregister')" if c=="type" else "decimal(16,2)")) for c in P],
      [(1,"product",1,1,"0.00","0.00","0.00","0.00","0.00","0.00","4.99","14.50","28.00","49.90","-1.00","-1.00"),
       (2,"product",2,1,"0.00","0.00","0.00","0.00","0.00","0.00","4.59","-1.00","-1.00","45.90","-1.00","-1.00"),
       (3,"product",1,2,"10.00","0.00","0.00","0.00","0.00","0.00","9.99","-1.00","-1.00","99.00","180.00","-1.00"),
       (4,"product",1,3,"0.00","0.00","0.00","0.00","0.00","0.00","19.00","-1.00","-1.00","-1.00","-1.00","-1.00"),
       (5,"product",1,4,"25.00","-1.00","-1.00","-1.00","-1.00","-1.00","0.00","-1.00","-1.00","-1.00","-1.00","-1.00"),
       (6,"addon",1,1,"0.00","0.00","0.00","0.00","0.00","0.00","2.00","-1.00","-1.00","20.00","-1.00","-1.00"),
       (7,"configoptions",1,2,"0.00","0.00","0.00","0.00","0.00","0.00","5.00","-1.00","-1.00","50.00","-1.00","-1.00"),
       (8,"domainregister",1,1,"0.00","0.00","0.00","0.00","0.00","0.00","9.99","19.98","-1.00","-1.00","-1.00","-1.00")])
table("tblproductconfiggroups", [("id","int(10)"),("name","text"),("description","text")], [(1,"VPS options @T@","")])
table("tblproductconfiglinks", [("id","int(10)"),("gid","int(10)"),("pid","int(10)")], [(1,1,3)])
table("tblproductconfigoptions", [("id","int(10)"),("gid","int(10)"),("optionname","text"),("optiontype","text"),("qtyminimum","int(10)"),("qtymaximum","int(10)"),("order","int(1)"),("hidden","tinyint(1)")],
      [(1,1,"ram|Memory","1",0,0,1,0)])
table("tblproductconfigoptionssub", [("id","int(10)"),("configid","int(10)"),("optionname","text"),("sortorder","int(10)"),("hidden","tinyint(1)")],
      [(1,1,"2048|2 GB",1,0),(2,1,"4096|4 GB",2,0)])
table("tbladdons", [("id","int(10)"),("packages","text"),("name","text"),("description","text"),("billingcycle","text"),("tax","tinyint(1)"),("showorder","tinyint(1)"),("hidden","tinyint(1)"),("retired","tinyint(1)"),("weight","int(10)")],
      [(1,"1,2","Daily Backups @T@","Off-site backups","recurring",1,1,0,0,1)])
table("tblservers", [("id","int(10)"),("name","text"),("ipaddress","text"),("assignedips","text"),("hostname","text"),("monthlycost","decimal(16,2)"),("noc","text"),("statusaddress","text"),("nameserver1","text"),("maxaccounts","int(10)"),("type","text"),("username","text"),("password","text"),("accesshash","text"),("secure","text"),("port","int(8)"),("active","int(1)"),("disabled","int(1)")],
      [(1,"WHM-01 @T@","203.0.113.10","","whm01.example.net","40.00","","","ns1.example.net",200,"cpanel","root",SRV_PW,"","on",2087,1,0),
       (2,"Virt-01 @T@","203.0.113.20","","","0.00","","","",0,"virtualizor","apikey","","VIRT-ACCESS-HASH-123","on",4085,1,0),
       (3,"Old box @T@","","","","0.00","","","",0,"cpanel","root","","","",0,0,1)])
table("tblhosting", [("id","int(10)"),("userid","int(10)"),("orderid","int(10)"),("packageid","int(10)"),("server","int(10)"),("regdate","date"),("domain","text"),("paymentmethod","text"),("firstpaymentamount","decimal(16,2)"),("amount","decimal(16,2)"),("billingcycle","text"),("nextduedate","date"),("nextinvoicedate","date"),("domainstatus","enum('Pending','Active','Suspended','Terminated','Cancelled','Fraud','Completed')"),("username","text"),("password","text"),("notes","text"),("dedicatedip","text")],
      [(1,1,1,1,1,"2023-05-14","ana-@T@.ro","stripe","4.99","4.99","Monthly","2024-03-14","2024-03-14","Active","anapop",enc(CC,"acct-pass",bytes(20)),"","") ,
       (2,2,2,2,1,"2015-03-02","obrien-@T@.ie","paypal","120.00","99.00","Annually","2025-03-02","2025-03-02","Suspended","obrienw","","overdue",""),
       (3,1,3,3,2,"2024-01-10","vps-@T@.example","stripe","19.00","24.00","Monthly","2024-02-10","2024-02-10","Active","","","","203.0.113.50"),
       (4,3,4,4,0,"2020-01-01","","banktransfer","25.00","0.00","Free Account","0000-00-00","0000-00-00","Terminated","","","","")])
table("tblhostingaddons", [("id","int(10)"),("orderid","int(10)"),("hostingid","int(10)"),("addonid","int(10)"),("name","text"),("setupfee","decimal(16,2)"),("recurring","decimal(16,2)"),("billingcycle","text"),("status","enum('Pending','Active','Suspended','Terminated','Cancelled','Fraud','Completed')"),("regdate","date"),("nextduedate","date")],
      [(1,1,1,1,"","0.00","2.00","Monthly","Active","2023-05-14","2024-03-14")])
table("tblhostingconfigoptions", [("id","int(10)"),("relid","int(10)"),("configid","int(10)"),("optionid","int(10)"),("qty","int(10)")], [(1,3,1,2,0)])
table("tblcustomfields", [("id","int(10)"),("type","text"),("relid","int(10)"),("fieldname","text"),("fieldtype","text"),("description","text"),("fieldoptions","text"),("regexpr","text"),("adminonly","text"),("required","text"),("showorder","text"),("showinvoice","text"),("sortorder","int(10)")],
      [(1,"product",1,"Website purpose","dropdown","","Blog,Shop,Other","","","on","on","",0)])
table("tblcustomfieldsvalues", [("id","int(10)"),("fieldid","int(10)"),("relid","int(10)"),("value","text")], [(1,1,1,"Shop")])
table("tbldomains", [("id","int(10)"),("userid","int(10)"),("orderid","int(10)"),("type","enum('Register','Transfer')"),("registrationdate","date"),("domain","text"),("firstpaymentamount","decimal(16,2)"),("recurringamount","decimal(16,2)"),("registrar","text"),("registrationperiod","int(1)"),("expirydate","date"),("subscriptionid","text"),("status","enum('Pending','Pending Registration','Pending Transfer','Active','Grace','Redemption','Expired','Cancelled','Fraud','Transferred Away')"),("nextduedate","date"),("nextinvoicedate","date"),("dnsmanagement","text"),("emailforwarding","text"),("idprotection","text"),("donotrenew","text")],
      [(1,1,1,"Register","2023-05-14","ana-@T@.ro","9.99","9.99","rotld",1,"2025-05-14","","Active","2025-05-14","2025-04-30","on","","","") ,
       (2,2,2,"Transfer","2015-03-02","obrien-@T@.ie","0.00","19.98","",2,"0000-00-00","","Pending Transfer","2025-03-02","2025-03-02","","","on","on")])
table("tblinvoices", [("id","int(10)"),("userid","int(10)"),("invoicenum","text"),("date","date"),("duedate","date"),("datepaid","datetime"),("last_capture_attempt","datetime"),("subtotal","decimal(16,2)"),("credit","decimal(16,2)"),("tax","decimal(16,2)"),("tax2","decimal(16,2)"),("total","decimal(16,2)"),("taxrate","decimal(10,3)"),("taxrate2","decimal(10,3)"),("status","text"),("paymentmethod","text"),("notes","text")],
      [(1001,1,"@T@-1001","2024-01-14","2024-01-21","2024-01-15 08:12:44","0000-00-00 00:00:00","6.99","0.00","1.33","0.00","8.32","19.000","0.000","Paid","stripe",""),
       (1002,1,"@T@-1002","2024-02-14","2024-02-21","0000-00-00 00:00:00","0000-00-00 00:00:00","6.99","5.00","1.33","0.00","3.32","19.000","0.000","Unpaid","stripe",""),
       (1003,2,"","2024-03-02","2024-03-09","0000-00-00 00:00:00","0000-00-00 00:00:00","99.00","0.00","0.00","0.00","99.00","0.000","0.000","Cancelled","paypal","duplicate")])
table("tblinvoiceitems", [("id","int(10)"),("invoiceid","int(10)"),("userid","int(10)"),("type","varchar(30)"),("relid","int(10)"),("description","text"),("amount","decimal(16,2)"),("taxed","int(1)"),("duedate","date"),("paymentmethod","text"),("notes","text")],
      [(1,1001,1,"Hosting",1,"Starter - ana-@T@.ro (14/01/2024 - 13/02/2024)","4.99",1,"2024-01-21","stripe",""),
       (2,1001,1,"Addon",1,"Daily Backups","2.00",1,"2024-01-21","stripe",""),
       (3,1002,1,"Hosting",1,"Starter - ana-@T@.ro (14/02/2024 - 13/03/2024)","4.99",1,"2024-02-21","stripe",""),
       (4,1002,1,"Addon",1,"Daily Backups","2.00",1,"2024-02-21","stripe",""),
       (5,1003,2,"Hosting",2,"Pro - obrien-@T@.ie","99.00",0,"2024-03-09","paypal","")])
table("tblaccounts", [("id","int(10)"),("userid","int(10)"),("currency","int(10)"),("gateway","text"),("date","datetime"),("description","text"),("amountin","decimal(16,2)"),("fees","decimal(16,2)"),("amountout","decimal(16,2)"),("rate","decimal(10,5)"),("transid","text"),("invoiceid","int(10)"),("refundid","int(10)")],
      [(1,1,0,"stripe","2024-01-15 08:12:44","Invoice Payment","8.32","0.54","0.00","1.00000","ch_@T@_1",1001,0),
       (2,1,0,"stripe","2024-01-20 10:00:00","Invoice Payment","0.00","0.00","2.00","1.00000","re_@T@_1",1001,1)])
table("tblcredit", [("id","int(10)"),("clientid","int(10)"),("date","date"),("description","text"),("amount","decimal(16,2)"),("relid","int(10)")],
      [(1,1,"2023-12-01","Goodwill credit","30.00",0),(2,1,"2024-02-14","Credit Applied to Invoice #1002","-5.00",1002)])
table("tblticketdepartments", [("id","int(10)"),("name","text"),("description","text"),("email","text"),("clientsonly","text"),("piperepliesonly","text"),("noautoresponder","text"),("hidden","text"),("order","int(1)")],
      [(1,"Support @T@","Technical help","support@example.net","","","","",1),(2,"Billing @T@","Invoices","billing@example.net","on","","","",2)])
table("tbltickets", [("id","int(10)"),("tid","varchar(128)"),("did","int(10)"),("userid","int(10)"),("contactid","int(10)"),("name","text"),("email","text"),("c","text"),("date","datetime"),("title","text"),("message","text"),("status","varchar(64)"),("urgency","text"),("admin","text"),("lastreply","datetime"),("flag","int(1)")],
      [(1,"ABC-123456",1,1,0,"","","x1y2z3","2024-01-20 09:00:00","Site down @T@","My site returns 500 since this morning.\r\nPlease help!","Answered","High","","2024-01-20 09:30:00",0),
       (2,"DEF-654321",2,2,0,"","","q9w8e7","2024-03-03 12:00:00","Refund @T@","Please cancel invoice 1003.","Closed","Medium","","2024-03-03 13:00:00",0)])
table("tblticketreplies", [("id","int(10)"),("tid","int(10)"),("userid","int(10)"),("contactid","int(10)"),("name","text"),("email","text"),("date","datetime"),("message","text"),("admin","text"),("attachment","text"),("rating","int(5)")],
      [(3,2,0,0,"","","2024-03-03 13:00:00","Done, invoice cancelled.","Jane Admin","",0),
       (1,1,0,0,"","","2024-01-20 09:15:00","Looking into it now.","Jane Admin","",0),
       (2,1,1,0,"","","2024-01-20 09:30:00","It works again, thanks!","","",5)])
table("tblannouncements", [("id","int(10)"),("date","datetime"),("title","text"),("announcement","text"),("published","int(1)"),("parentid","int(10)"),("language","text")],
      [(1,"2024-01-01 00:00:00","Happy new year @T@","<p>Our <b>new</b> plans are live.</p>",1,0,""),(2,"2024-01-01 00:00:00","La multi ani","<p>…</p>",1,1,"romanian")])
table("tblknowledgebasecats", [("id","int(10)"),("parentid","int(10)"),("name","text"),("description","text"),("hidden","text"),("catid","int(10)"),("language","text")],
      [(1,0,"Getting started @T@","Basics","",0,""),(2,0,"Primii pasi","","",1,"romanian")])
table("tblknowledgebase", [("id","int(10)"),("title","text"),("article","text"),("views","int(10)"),("votes","int(10)"),("useful","int(10)"),("order","int(3)"),("parentid","int(10)"),("language","text"),("private","text")],
      [(1,"How to upload files @T@","Use FTP or the file manager.",42,3,3,1,0,"",""),(2,"Cum incarc fisiere","Folositi FTP.",1,0,0,1,1,"romanian","")])
table("tblknowledgebaselinks", [("id","int(10)"),("categoryid","int(10)"),("articleid","int(10)")], [(1,1,1)])
table("tblpromotions", [("id","int(10)"),("code","text"),("type","text"),("recurring","int(1)"),("value","decimal(16,2)"),("cycles","text"),("appliesto","text"),("requires","text"),("requiresexisting","int(1)"),("startdate","date"),("expirationdate","date"),("maxuses","int(10)"),("uses","int(10)"),("lifetimepromo","int(1)"),("applyonce","int(1)"),("newsignups","int(1)"),("existingclient","int(1)"),("onceperclient","int(1)"),("recurfor","int(3)"),("upgrades","int(1)"),("upgradeconfig","text"),("notes","text")],
      [(1,"SPRING@T@","Percentage",1,"20.00","","1,2","",0,"2024-03-01","2024-05-31",100,7,0,0,1,0,0,0,0,"","Spring sale"),
       (2,"FIVEOFF@T@","Fixed Amount",0,"5.00","","","",0,"0000-00-00","0000-00-00",0,3,0,0,0,0,0,0,0,"",""),
       (3,"FREESETUP@T@","Free Setup",0,"0.00","","1","",0,"0000-00-00","0000-00-00",0,0,0,0,0,0,0,0,0,"",""),
       (4,"PRICE@T@","Price Override",0,"1.00","","1","",0,"0000-00-00","0000-00-00",0,0,0,0,0,0,0,0,0,"","")])
table("tbltax", [("id","int(10)"),("level","int(1)"),("name","text"),("state","text"),("country","text"),("taxrate","decimal(10,3)")],
      [(1,1,"VAT RO @T@","","RO","19.000"),(2,1,"MwSt AT @T@","","AT","20.000"),(3,2,"State tax @T@","CA","US","7.250")])

def lit(v):
    if v is None: return "NULL"
    if isinstance(v, int): return str(v)
    if isinstance(v, str) and v.startswith("0x") and all(c in "0123456789ABCDEFabcdef" for c in v[2:]) and len(v) > 2: return v
    s = v.replace("\\", "\\\\").replace("'", "\\'").replace('"', '\\"').replace("\n", "\\n").replace("\r", "\\r")
    return "'" + s + "'"

out = ["-- MySQL dump 10.13  Distrib 8.0.36, for Linux (x86_64)", "--", "-- Host: localhost    Database: whmcs", "-- ------------------------------------------------------", "-- Server version\t8.0.36", "--",
       "-- Synthetic WHMCS 8.10 export used by PeltyCMS tests (tests/sqldump_fixture.rs).",
       "-- @T@ is replaced by a unique tag per test run. cc_encryption_hash: " + CC,
       "-- Card columns / tblcreditcards hold fake values: the importer must never read them.", "",
       "/*!40101 SET @OLD_CHARACTER_SET_CLIENT=@@CHARACTER_SET_CLIENT */;", "/*!50503 SET NAMES utf8mb4 */;", "/*!40014 SET @OLD_UNIQUE_CHECKS=@@UNIQUE_CHECKS, UNIQUE_CHECKS=0 */;", ""]
for name, (cols, rows) in T.items():
    out += ["--", f"-- Table structure for table `{name}`", "--", "", f"DROP TABLE IF EXISTS `{name}`;", "/*!40101 SET @saved_cs_client     = @@character_set_client */;", f"CREATE TABLE `{name}` ("]
    defs = []
    for c, ty in cols:
        if c == "id": defs.append(f"  `id` {ty} unsigned NOT NULL AUTO_INCREMENT")
        else: defs.append(f"  `{c}` {ty} DEFAULT NULL" if "text" in ty or "blob" in ty else f"  `{c}` {ty} NOT NULL DEFAULT '{'0' if 'int' in ty else ''}'" if 'enum' not in ty and 'date' not in ty and 'time' not in ty else f"  `{c}` {ty} DEFAULT NULL")
    if cols[0][0] == "id": defs.append("  PRIMARY KEY (`id`)")
    else: defs.append(f"  KEY `{cols[0][0]}` (`{cols[0][0]}`(32))")
    out.append(",\n".join(defs)); out.append(") ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;")
    out += ["/*!40101 SET character_set_client = @saved_cs_client */;", "", "--", f"-- Dumping data for table `{name}`", "--", "", f"LOCK TABLES `{name}` WRITE;", f"/*!40000 ALTER TABLE `{name}` DISABLE KEYS */;"]
    out.append(f"INSERT INTO `{name}` VALUES " + ",".join("(" + ",".join(lit(v) for v in r) + ")" for r in rows) + ";")
    out += [f"/*!40000 ALTER TABLE `{name}` ENABLE KEYS */;", "UNLOCK TABLES;", ""]
out += ["/*!40014 SET UNIQUE_CHECKS=@OLD_UNIQUE_CHECKS */;", "/*!40101 SET CHARACTER_SET_CLIENT=@OLD_CHARACTER_SET_CLIENT */;", "", "-- Dump completed on 2024-03-15 12:00:00", ""]
open("/workspace/PeltyCMS/crates/pelty-import/tests/fixtures/whmcs.sql", "w").write("\n".join(out))
print(SRV_PW, BOB_MD5)
