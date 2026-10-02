---
title: Frequently asked questions
description: Cloud coexistence, independence, certificates, and common expectations.
---
## Does Handy still work?

That is the purpose of the project: the physical printer remains cloud-paired while LAN clients use the virtual endpoint. Handy continuity was confirmed during the supervised release test. Cloud, account, and firmware changes can still affect compatibility.

## Is this Bambu Gateway, or does it require it?

No. The Rust application and isolated C++ plugin adapter are maintained here. The Compose core service and state directory retain the historical name `gateway`, but there is no Bambu Gateway runtime dependency.

The official proprietary Bambu networking plugin is downloaded from Bambu at startup and checked against reviewed version/hash pins. See the repository's `THIRD_PARTY_NOTICES` for dependency notices.

## Is it entirely open source?

The project code is MIT-licensed. The official Bambu plugin is proprietary and not relicensed by this project. Cloud service use remains subject to Bambu's terms and policies.

## Do I need to extract certificates manually?

Normal installation creates a local proxy CA and downloads/initializes the official plugin; account/device signing material is managed in private plugin state. You do not need an external Gateway installation.

The public CA for client TLS trust is different from the plugin's private cloud device identity and from upstream camera certificate pins. Share only the public CA export.

## Is it fully offline?

No. Local reads may be available, but cloud-signed control and print submission require Bambu Cloud. This is a coexistence bridge, not a cloud-free firmware replacement.

## Is read-only cloud access different from signed control?

Yes. Reports can arrive through cloud telemetry without giving a client permission to issue signed mutations. The proxy can also prefer local telemetry when the printer exposes it. Read-only use does not imply that arbitrary cloud writes are allowed.

## Can I use both real LAN and cloud mode without this proxy?

Some printer/firmware configurations expose local services while cloud-paired. Availability differs by protocol and firmware, and a local monitoring connection does not prove signed LAN control is available. Use official local services directly where they meet your needs.

## Can every printer have its own DHCP lease?

Not in the current implementation. It uses reserved static host aliases. See [networking](/docs/networking/).

## Is the file service SFTP?

No. Staging is implicit FTPS; printer-resident browsing uses Bambu's TLS native file protocol. Neither is SSH/SFTP.

## Can I publish the admin console?

Use a trusted HTTPS reverse proxy with local/OIDC authentication or a VPN/SSH tunnel. Do not expose printer ports. The public documentation site is completely separate and never connects to your printer APIs.

## Does the console include live video and print controls?

No. It manages connections, account access, capabilities, and policy. Use Bambuddy or another compatible client for live print workflows.
