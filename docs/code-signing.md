# Code signing

Goal: sign `bloop.exe` and `bloop-setup-<version>.exe` so Windows and antivirus tools can tell
who published them. The route is SignPath Foundation's free signing for open-source projects
(decision recorded in `docs/architecture.md`, 2026-11-11).

Public page: `site/code-signing.html` (<https://laurentmaxhuni.github.io/bloop/code-signing.html>),
linked as "Code signing policy" from the site footer, the README and every release's notes.

## How it fits together

`installer/release.ps1` pushes the tag `v<version>`. `.github/workflows/release.yml` then runs on
`windows-latest`:

1. fmt, clippy and tests, then `cargo build --release --locked`;
2. **if signing is configured:** upload `bloop.exe` as an artifact, submit it to SignPath
   (artifact configuration `exe`), wait for the manual approval, put the signed file back;
3. build the installer around that `bloop.exe` (`installer\build.ps1 -SkipCargo`);
4. **if signing is configured:** submit the installer (artifact configuration `installer`) the
   same way;
5. write the `.sha256` and publish the GitHub release.

Signing is configured when the repository has the secret `SIGNPATH_API_TOKEN` and the variable
`SIGNPATH_ORGANIZATION_ID`. Without them (and on pull requests) the same steps run unsigned, which
is how releases are published until SignPath approves the project. Each signing step fails the
run if `Get-AuthenticodeSignature` doesn't report a valid signature.

`build.rs` embeds a version resource (`ProductName` "bloop", `ProductVersion` from `Cargo.toml`),
so SignPath's metadata restrictions can require the same product name in every build. Inno Setup
sets the installer's version info from `AppName` and `AppVersion`.

## Setup checklist (the maintainer does these; they are tied to your identity)

1. **Two-factor authentication** on GitHub for everyone who can push (SignPath requires it).
2. **Apply** at <https://signpath.org> for the free open-source certificate. Give the repository
   URL, the code signing policy page and the release workflow. The project must already have
   releases, which it does (v0.3.0 onwards).
3. **After approval, in SignPath:**
   - create the project with slug `bloop` and connect GitHub as its trusted build system for
     `LaurentMaxhuni/bloop`;
   - add two artifact configurations: `exe` (a zip containing `bloop.exe`, sign it as a PE file)
     and `installer` (a zip containing `bloop-setup-*.exe`, sign it as a PE file);
   - add the signing policy `release-signing` with manual approval by the approvers named on the
     policy page, and set metadata restrictions to product name `bloop`;
   - create an API token for a CI user that may submit to that policy.
4. **In GitHub › Settings › Secrets and variables › Actions:**
   - secret `SIGNPATH_API_TOKEN`: the API token;
   - variable `SIGNPATH_ORGANIZATION_ID`: your SignPath organization ID;
   - optional variables `SIGNPATH_PROJECT_SLUG` (default `bloop`) and
     `SIGNPATH_SIGNING_POLICY_SLUG` (default `release-signing`) if you chose other slugs.
5. **Release** as usual with `installer\release.ps1`. Approve the two signing requests in SignPath
   when the run pauses on them (each waits up to 10 minutes).
6. **Update the policy page:** remove the "Status" paragraph from `site/code-signing.html` and the
   "isn't code-signed yet" lines from the README and site FAQ once a signed release is out.

## Limits

- The uninstaller (`unins000.exe`) that Inno Setup writes during installation stays unsigned:
  Inno can only sign it by running a signing tool on the build machine during compilation.
- The certificate names SignPath Foundation as the publisher, not you.
- A new certificate still has to build SmartScreen reputation, so warnings can continue for a
  while after the first signed release. Signing lowers the chance of antivirus false positives but
  doesn't rule them out; report a specific detection to the vendor (for Microsoft Defender, the
  file submission portal).
- `release.ps1 -Local` builds on your PC and is never signed.
