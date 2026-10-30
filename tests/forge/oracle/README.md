# Event-bus oracle provenance

`bus-client.jsonl` is a captured real Forge 1.8.9 client trace. On 2026-10-30,
four offline, headless Forge 1.8.9 dedicated-server runs used the original FTBus
source from commit `7fab035`, with only its `clientSideOnly` annotation changed
from `true` to `false`; the test handlers and their event code were unchanged.
Runs 0 and 3 matched the client trace byte for byte. Runs 1 and 2 produced the
identical `bus-server-instance.jsonl` trace, which contains an additional EV-05
instance-handler record and consequently incremented later sequence numbers.
Both orders are consistent with Forge registering same-priority handlers in
`Class.getMethods()` order before the static-handler invocation throws. Server
behavior is evidence for those two orders, not proof of every possible client
ordering. The probe compares the full raw trace against exactly these two files.
