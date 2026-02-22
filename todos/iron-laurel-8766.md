---
id: iron-laurel-8766
created_at: 2026-02-22T11:52:19.825697Z
status: open
summary: Multiple LLM providers
---
We need to create an abstraction and then test with multiple providers. Perhaps there's a common REST API shape that we can use? Or does it make sense to link multiple providers, using official crates, via a multi-crate architecture where each provider gets its own crate, and we choose at build time which to include via features?