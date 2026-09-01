---
type: User Profile
title: Vault user profile
description: Canonical identity and stable profile of the person this vault serves.
owner:
  actor: null
  names: []
  confirmed: false
status: draft
---

# Vault user profile

This file is the canonical, machine-readable answer to "whose vault is this?"
Humans and agents may maintain it together, but a new vault starts deliberately
unclaimed.

## Owner activation

Replace `owner.actor` with one stable `human:<id>` actor, add the names that
sources may use for that person, and set `owner.confirmed: true` only after the
person explicitly confirms the identity. Until all three conditions hold, an
agent must treat the owner as unknown and must not create owner Tasks.

## Stable profile

Keep only durable facts that help an agent work with the owner: preferred forms
of address, languages, timezone, stable responsibilities, and enduring working
preferences. Do not copy daily events, transient mood, reminders, credentials,
tokens, private keys, or secrets here.

For an agent-authored profile fact, include its evidence next to the claim:

```text
- <stable profile fact>
  source:: <vault-absolute path or URL>
  updated:: <RFC 3339 datetime>
  by:: <producer/version>
```

## Maintenance contract

- A human may edit this file directly.
- An agent may add or correct a stable profile fact only when it has a named
  source and records `source::`, `updated::`, and `by::` beside the claim.
- An agent must not change `owner.actor`, owner names, authority, permissions,
  or an action-sensitive preference without the owner's explicit confirmation.
- Resolve contradictions in place. Keep one active value and record the old
  value under a clearly marked superseded note rather than leaving two active
  truths.
- This file is normally synced with the vault. Do not put secrets here.
