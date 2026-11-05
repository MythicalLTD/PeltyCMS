# Security: 2FA, passkeys, sessions

## Two-factor authentication

Two-factor authentication, passkeys and your security question are on the **Sessions** page (`/sessions`). To enable two-factor authentication:

- Scan the QR code with an authenticator app (Google Authenticator, 1Password, Authy and others) and enter the 6-digit code to confirm.
- Save the **backup codes** somewhere safe. Each one works once instead of a code, if you lose your phone.

From then on, signing in asks for a code after your password. You can create new backup codes or disable 2FA after confirming it is you.

## Passkeys

A passkey signs you in with your device (fingerprint, face or PIN) instead of a password. Add one on **Sessions**; you can add several and remove them. If you lose the device, sign in with your password (and 2FA code or backup code) and remove the old passkey.

**Passkeys for two-step verification.** If you use an authenticator app, you can answer the second step with a passkey instead of a code. Without an app, turn on **Use passkeys for two-step verification** on **Sessions**: every password sign-in will then ask for one of your passkeys. Keep at least two passkeys, because the last one can't be removed while it is your only second factor. If you lose all of them, support can reset your passkeys.

## Sessions

The same page lists where you are signed in, with device and IP address. Sign out any session you do not recognise, then change your password.

## Security question

Your provider may ask you to set a security question, used to confirm who you are when you contact support.

{{routes:/auth/mfa,/auth/mfa/passkey,/auth/passkeys,/auth/sessions,/auth/password}}
