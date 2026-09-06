# Routeless and brokered by default

A manifest that says nothing about access now gets `network = "none"`
and, when it has an agent, the broker — spawned by the box start itself,
with that manifest's `egress` allowlist, its socket in the box's own
directory, ended with the box. The old default handed every box the
host's whole network and asked the user to grant the credential file,
which made the design's central claim ("no credential in box, no route")
an opt-in that daily use never exercised.

The broker gained a `CONNECT` leg for everything that is not the model
API: `HTTPS_PROXY` points the box's tools at the forwarder, the broker
judges each host against `[access] egress` (exact names, one-level
wildcards only beside their base), resolves and dials host-side, and
relays without interception. Ports 443 and 80 only. A blocked host is a
`403` naming the fix, never a hang.

Considered and rejected: a shared host-wide broker (one socket cannot
carry per-box policy, and revocation must be per box); auto-adding
`rustup target`-style credentials or interactive prompts at launch
(asking happens at install or in `ask`, never at launch); proxying plain
HTTP (packages belong in the image; the build box keeps `dns` for its
own fetches, which is why `dns` beside `network = "none"` is legal and
feeds the build alone).
