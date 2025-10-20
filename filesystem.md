# Semantic Filesystem (AI-FS) — design doc

---

## Principles & constraints

* **Data integrity first:** The primary file data path is standard (block-backed, checksummed). Semantic layers must not alter primary blocks.
* **Local-first & privacy-aware:** Indexing and embeddings default to local-only. Any off-host export explicitly requires operator consent and redaction.
* **Auditable & reversible:** Every automatic enrichment, index update, and repair action is logged with provenance and can be reverted via snapshots.
* **Pluggable models:** Start with lightweight, CPU-friendly extractors (TF-IDF, n-gram) and allow swapping to larger quantized models later.
* **Incremental & lazy indexing:** Avoid blocking the write path — enqueue enrichment jobs and update indexes asynchronously under resource limits.

---

## High-level components

1. **Block layer** — existing block device driver (virtio-blk / AHCI).
2. **Core FS (Data Store)** — copy-on-write, journaling metadata, primary file content, checksums. (Think a simple inode + block map or an append-log-backed layout.)
3. **Snapshot Manager** — atomic snapshot/rollback primitives for data + metadata.
4. **Semantic Engine**

   * **Extractor**: transforms file bytes to text/tokens/images→features.
   * **Indexer**: vector index (embeddings) + metadata DB.
   * **RAG Retriever**: query interface returning ranked file candidates + context.
5. **Repair/Forensics** — corruption detector, auto-repair suggestions, and rollback orchestrator.
6. **Policy & Audit** — policy engine for privacy, retention, and AI autonomy; append-only audit log.
7. **Operator UI / CLI** — query/search, approve repairs, manage models.

---

## On-disk layout (logical — concrete byte layout later)

* `superblock` — FS magic, version, root inode location, pointers to index areas, policy pointer, UUID.
* `inode_table` — inodes (mode, owner, timestamps, block pointers, extended attrs pointer).
* `data_blocks` — content blocks (content-addressed optional: store checksum and block ref).
* `journal` — write-ahead log for metadata/txns.
* `snapshots` — snapshot metadata and pointers (immutable).
* `semantic_meta` (separate region or file): append-only log of metadata records:

  * Records: `{record_id, file_id, offset, op_type, timestamp, extractor_version, summary, tags[], embedding_id}`
* `embeddings_store` — flat store of vectors (float16/quantized int8) with id → offset mapping.
* `index_shard` — nearest neighbor index structure (HNSW or IVF on-disk shards) referencing `embeddings_store`.
* `audit_log` — append-only, signed/sealed entries for events/actions.
* `policy_store` — JSON/DSL policy documents (retention, redaction, access rules).

Notes:

* Keep semantic regions optional and mountable/unmountable. Basic FS should operate if semantic regions are missing.
* Use checksums on all critical metadata regions.

---

## Semantic record model

Each enrichment produces a `SemanticRecord`:

```
{
  record_id: UUID,
  file_id: FileID,
  file_path: canonical_path,
  byte_range: [start, end],
  extractor: "tfidf-v1" | "emb-quant-1",
  summary: "Short sentence summarizing content",
  tags: ["kernel","rust","uefi"],
  embedding_id: EMB_ID,
  provenance: { model_version, command_hash, timestamp, host_id },
  privacy_flags: { pii_detected: bool, redacted: bool, redaction_mask: ... }
}
```

Embeddings are stored separately and referenced by `embedding_id`.

---

## Indexing & ingestion pipeline

Design for asynchronous ingestion:

1. **Write path (synchronous)**

   * Write file blocks → update inode → journal commit.
   * Append a lightweight `IndexJob` record to the `semantic_queue` (file_id, op, priority).

2. **Index worker(s) (userland or kernel worker)**

   * Pull job → read file segments → pass to Extractor(s).
   * Extractor returns summary, tags, small features, and embedding vector.
   * Append `SemanticRecord` to `semantic_meta`.
   * Store embedding to `embeddings_store`; update `index_shard` (HNSW insert or batch IVF build).

3. **Consistency & failure**

   * If worker crashes or fails, job remains in queue. Add retry/backoff.
   * Index update is idempotent (each record has unique record_id); re-indexing creates new record version.

4. **Resource governance**

   * Scheduler enforces CPU/memory limits for workers.
   * Background indexing uses low I/O priority; allow operator to pause.

---

## Query & retrieval API

Expose two principal interfaces:

1. **Semantic search (userland API / RPC)**

   * `search(query_text, top_k, filters, snapshot_id?) -> [ {file_id, score, snippet, record_meta} ]`
   * Internals: embed query → NN search in `index_shard` → re-rank using summary/tags/TF-IDF score and policies.

2. **File retrieval / context**

   * `get_file_context(file_id, record_id, context_bytes) -> bytes`
   * Returns piece of file and semantic metadata.

3. **RAG pipeline helper**

   * `retrieve_for_rag(query, top_k, max_tokens) -> concatenated_docs` (returns small, provenance-tracked documents for augmentation).

All responses include `provenance` for audit.

---

## Snapshot, repair & self-healing

* **Snapshot semantics:** snapshot is atomic view over `inode_table`, `data_blocks`, and `semantic_meta` pointers. Snapshot creation is cheap (copy-on-write metadata) and fast for shell rollback.
* **Repair model:** detector regularly scans checksum mismatches / failing reads and records a `corruption_event`.

  * Repair strategy:

    1. Try `semantic_restore`: use nearest previous snapshot & semantic hints to suggest probable content (e.g., previous summary + diffs).
    2. If snapshot exists, orchestrator offers `rollback_file(file_id, snapshot_id)` or `rollback_fs(snapshot_id)`.
    3. If partial corruption, use `delta_reconstruction` via semantic similarity to other files (for derived files). This is only a suggestion — operator must approve before applying.
* **Simulator dry-run:** test repairs in an isolated micro-VM using snapshot copy. Only after tests pass and policies allow, action is applied.

---

## Security & privacy

* **Access control:** file ACLs + policy engine enforces which users/processes can read semantic metadata or request embeddings.
* **Redaction:** PII detector flags sensitive segments; system masks them before indexing/sharing.
* **Audit:** every automatic enrichment, search query, or repair gets logged: `{who, what, when, why, model_id, prompt_hash}` and signed.
* **Encryption:** embeddings and semantic_meta can be encrypted at rest; keys managed via TPM or kernel keystore. Different tenants can use distinct keys.
* **Model provenance:** models are signed and versioned; the metadata tracks `model_hash` and `model_owner`.

---

## Performance / storage considerations

* **Embedding size & quantization:** store int8/uint8 quantized vectors (plus scale) to reduce disk. Use product quantization for large datasets.
* **Shard & tier:** split index into shards by time or namespace; hot shards on SSD, cold shards on HDD or object storage.
* **Compaction & GC:** garbage collect orphan embeddings and old semantic records. Compaction rebuilds HNSW/IVF shards offline.
* **Memory use:** keep small in-memory caches (LRU) for recent embeddings + compressed representation.

---

## Consistency & crash safety

* **Write ordering:** commit primary data first (journal), then enqueue semantic job. If system crashes, primary data is safe and semantic queue will resume/update later.
* **Atomic index update:** use two-phase commit for embedding insert + index pointer switch: write embedding → append record → atomically update index pointer.
* **Recovery:** on mount, replay journal, reconcile semantic queue (remove records pointing to missing files), and rebuild index shards if shard checksum mismatched.

---

## APIs (high level)

### Kernel Syscalls / RPCs

* `fs_open(path, flags) -> handle`
* `fs_read(handle, buf, offset) -> n`
* `fs_write(handle, buf, offset) -> n`
* `fs_snapshot_create(name) -> snapshot_id`
* `fs_snapshot_rollback(snapshot_id) -> result`
* `ai_index_request(file_id, range, priority) -> job_id`
* `ai_search(query, filters, top_k) -> results`
* `ai_get_record(record_id) -> SemanticRecord`

### Admin/Operator CLI

* `ai-fs status` — index health, pending jobs
* `ai-fs pause-indexing` / `resume-indexing`
* `ai-fs snapshot create NAME`
* `ai-fs repair suggest FILE` — returns suggested repairs and tests
* `ai-fs export-provenance JOB_ID` — produce signed audit bundle

---

## Testing & validation plan

1. **Unit tests (host):** extractor tokenization, tag rules, embedding serialization, redaction.
2. **Integration tests (QEMU):** write files → ensure index job created → search returns file.
3. **Crash tests:** force mid-index crash; on remount ensure system recovers and index jobs resume.
4. **Privacy tests:** create files containing PII; ensure they’re flagged and redacted for index/export.
5. **Repair test:** corrupt a block, run detector, suggest a repair, simulate repair in micro-VM snapshot, apply on approval, verify data integrity.
6. **Performance tests:** measure indexing throughput, search latency, and memory usage under load.

---

## Operational & governance notes

* **Default settings:** indexing disabled for `/private`, opt-in per dataset.
* **Quota policies:** per-user embedding/index quota; evict old embeddings by LRU + semantic similarity.
* **Model lifecycle:** model registry (versioned), sign model binaries; allow operator to pin model for a tenant.
* **Audit retention:** store provenance for a minimum retention period; auto-rotate and archive.

---

## Phased roadmap (practical)

**Phase 0 (MVP)** — weeks

* Simple FS (append-only or simple inode) + semantic queue + lightweight TF-IDF extractor and tagger.
* On-disk `semantic_meta` log + test search that uses TF-IDF.

**Phase 1 (indexing + embeddings)** — 1–2 months

* Add embedding store and small HNSW in-memory shard with periodic persistence.
* Background index workers + basic resource limits.

**Phase 2 (privacy & snapshots)** — 2–4 months

* PII detector + redaction, snapshot/rollback primitive, audit logging.
* Simulator dry run integration.

**Phase 3 (production features)** — 4–8 months

* Quantized embeddings, sharding, tiered storage, signed models, admin UI.
* Integrate with repaird orchestrator for automatic suggestions (human-in-loop).

**Phase 4 (scale & federation)** — 8–18+ months

* Federated indices, cross-node RAG, model federation, distributed knowledge fabric.

---

## Operational checklist before first production run

* Mount FS in QEMU, ensure journal replay works.
* Index small corpus, run semantic search queries.
* Test snapshot create/rollback and recovery flow.
* Validate audit log signatures and policy enforcement.
* Confirm model pin/rollback works and model binaries are signed.

---

## Closing: developer notes & incremental tips

* **Start small:** implement extractor + semantic_meta log first; this is low risk and provides immediate value.
* **Keep primary data path minimal & fast.** The semantic layers are secondary and must not impact core FS guarantees.
* **Make index pluggable:** start with in-process HNSW for dev, later swap to on-disk or remote vector DB if needed.
* **Audit everything:** every automated action should be explainable — store prompt + outputs for reproducibility.

