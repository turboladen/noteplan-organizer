---
name: sync-rust-ts-types
description: Check and sync TypeScript API types with Rust model structs
disable-model-invocation: true
---

# Sync IPC Types

Compare Rust models to TypeScript types and fix any drift:

1. Read all structs with `#[derive(Serialize)]` from `src-tauri/src/models/`
2. Read all types from `src/types/api.ts`
3. Report any mismatches (missing fields, wrong types, missing types)
4. Update `src/types/api.ts` to match the Rust structs
5. Run `bunx tsc --noEmit` to verify the TypeScript still compiles

Type mapping: Rust → TypeScript
- `String` → `string`
- `usize`/`u32`/`i32`/`f64` → `number`
- `bool` → `boolean`
- `Vec<T>` → `T[]`
- `Option<T>` → `T | null`
- `HashMap<K,V>` → `Record<K, V>`
- `NaiveDateTime`/`DateTime` → `string` (ISO format)
