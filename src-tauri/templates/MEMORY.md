---
type: Memory
title: Shared long-term memory
description: Human-and-AI curated durable facts and decisions for this vault.
owner_ref: /USER.md
status: stable
---

# Shared long-term memory

Humans and agents maintain this file together. Keep it compact: it is the
curated durable layer, not a transcript, activity log, or dumping ground.

## Active memory

Record only facts, constraints, and decisions that are likely to matter across
future sessions and that do not belong in the owner's profile. Use one entry per
claim:

```text
- <durable fact, constraint, or decision>
  id:: <UUID v4>
  source:: <vault-absolute path or URL>
  recorded:: <RFC 3339 datetime>
  by:: <producer/version or human:id>
  verified:: <human:id at RFC 3339 datetime>   # optional
```

## Superseded memory

Move or copy an invalidated entry here, add `superseded:: <RFC 3339 datetime>`,
and link its replacement with `superseded-by:: <memory id>`. Do not silently
delete history that explains a current decision.

## Maintenance contract

- A human may edit this file directly.
- An agent may add or merge a durable entry only from a named source and must
  record `id::`, `source::`, `recorded::`, and `by::`.
- Owner identity and stable personal profile belong in `/USER.md`.
- Tasks and reminders belong in `/inbox/tasks/`; daily or episodic detail
  belongs in the vault's daily-note system. Raw transcripts stay with sources.
- Permission, authority, commitment, or other action-sensitive claims require
  explicit human confirmation before an agent treats them as active memory.
- On contradiction, preserve the prior entry as superseded and keep exactly one
  active claim. Periodically merge duplicates and remove obsolete detail whose
  provenance is no longer useful.
- This file is normally synced with the vault. Do not put secrets here, and do
  not expose its contents in shared, public, or external contexts unless the
  owner has authorized that use.
