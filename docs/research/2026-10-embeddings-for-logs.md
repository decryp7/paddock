# Do embeddings help plain-language log search? (October 2026)

Research note, not a decision. Debrief's "no vector search" decision stands unless the user changes it. The companion
decision for Pacenotes (code search) is Pacenotes ADR 0001, "Where Pacenotes' embeddings come from" (branch
`docs/embeddings-adr`), which has the speed, memory, size and licence measurements for the same engines.

## Question

Can a Debrief user find log events without knowing the exact keywords: "why did sign-in fail", "disk filling up", "app slow to
start", "lost connection to the database", where the logs say "auth: 401 token expired", "ENOSPC", "startup took 14.2 s",
"pool exhausted"? Which is the lightest option that makes such queries work well enough, alone or combined?

## Set-up

- **Events.** Debrief's generated samples only (`debrief/samples` at `77c45751`, no user logs): 36 text, JSON, CLEF and XML
  files, 44,018 lines. Events were reduced to distinct messages per file (digits and hex ids folded, CLEF templates rendered
  with their properties): **505 events** from 16 formats (Serilog, NLog, log4net, MEL, logcat, Apache, IIS, nginx, Spark,
  HDFS, kubelet/rsyslog, journald, Docker, Kubernetes pods, Debrief's monitor). Some messages are Japanese (log4net/NLog
  samples).
- **Queries (36), labelled by a pattern over the event text.** 30 **plain-language** queries with a wording gap ("disk filling
  up, no space left" -> `ディスクに空き容量がありません`; "the other side dropped the connection" -> "Connection reset by peer",
  "forcibly closed", "The underlying connection was closed"; "an order was received" -> `注文 88412 を受け付けました`), and 6
  **identifier** controls written the way users mistype them (`hikaripool`, `ordrservice paymnts`, `billingservice`,
  `sendordermail`, `payment adaptor circuitbreaker`, `order 88412`). 1 to 30 relevant events per query.
- **Methods.**
  - **BM25** over words, identifiers split at camelCase, `.`, `_`; CJK as character bigrams.
  - **names + BM25**: `paddock-names` (branch `shell/x18-names`) over the 254 identifiers seen in the events (246 entities
    after grouping): the query is resolved to at most two entities, events containing one of their spellings come first
    (ordered by BM25), then the BM25 list.
  - **Rewrite + BM25**, two ways. (1) A **fixed expansion table** of 31 generic user-word -> log-vocabulary rules (for
    example `disk|drive|space` -> `disk device I/O error ENOSPC "No space left on device" full`). It was written after looking
    at the sample vocabulary, so it is an **optimistic upper bound** for a rewriter that knows the data's words. (2) A **real
    local LLM rewrite**: Qwen3.5-2B (Q4_K_M) on llama.cpp `b11669`, temperature 0, a generic prompt asking for 8-15 terms likely
    to appear in log lines, no access to the data. This is what an unguided rewrite through Pitwall looks like. (3) The same
    model **grounded**: the prompt also lists the 829 words that occur in the events and asks it to pick from them.
  - **Embeddings**: Model2Vec `potion-code-16M-v2` and `potion-retrieval-32M` (`model2vec-rs`, pure Rust), BGE-small-en-v1.5
    (fastembed / ONNX Runtime, the quality reference asked for), and Qwen3-Embedding-0.6B (Pitwall's embedding model, through
    llama.cpp). Hybrids are reciprocal rank fusion (k = 60) of the top-100 lists.
- **Metrics**: recall@5 (relevant events in the top 5 over min(5, relevant)), hit@5 (queries with at least one relevant event
  in the top 5), MRR@10.

## Results

| Method | Plain recall@5 | Plain hit@5 | Plain MRR@10 | Identifier recall@5 | Identifier hit@5 |
|---|---|---|---|---|---|
| BM25 | 0.371 | 0.600 | 0.450 | 0.500 | 0.500 |
| names + BM25 | 0.371 | 0.600 | 0.450 | **0.967** | **1.000** |
| LLM rewrite (2B, unguided) + BM25 | 0.334 | 0.500 | 0.436 | 0.467 | 0.667 |
| LLM rewrite (2B, grounded in the event vocabulary) + BM25 | 0.333 | 0.600 | 0.431 | 0.533 | 0.667 |
| Table rewrite (upper bound) + BM25 | **0.701** | **0.900** | **0.783** | 0.433 | 0.500 |
| Model2Vec potion-code-16M-v2 | 0.429 | 0.600 | 0.522 | 0.800 | 0.833 |
| BM25 + potion-code-16M-v2 | 0.488 | 0.733 | 0.528 | 0.800 | 0.833 |
| BM25 + potion-retrieval-32M | 0.499 | 0.767 | 0.576 | 0.667 | 0.667 |
| Table rewrite + BM25 + potion-code-16M-v2 | 0.629 | 0.800 | 0.662 | 0.800 | 0.833 |
| Grounded 2B rewrite + BM25 + potion-code-16M-v2 | 0.533 | 0.767 | 0.529 | 0.800 | 1.000 |
| BGE-small | 0.519 | 0.700 | 0.570 | 0.800 | 1.000 |
| BM25 + BGE-small | 0.614 | 0.867 | 0.611 | 0.833 | 1.000 |
| Table rewrite + BM25 + BGE-small | 0.761 | 0.933 | 0.766 | 0.633 | 0.833 |
| Qwen3-Embedding-0.6B | **0.797** | **0.967** | **0.831** | 0.700 | 0.833 |
| Table rewrite + BM25 + Qwen3-Embedding | 0.838 | 0.967 | 0.853 | 0.733 | 0.833 |

Cost of each (Mac mini M4 Pro, measured in the Pacenotes spike, load in brackets): Model2Vec embeds these 505 events in
18-30 ms on one core (17-28k events/s, load 2), 15-44 ms model load, a 32 MB model, about 3 MB of binary; BGE-small 38
events/s on 13 threads (load 54), 0.5 s load, 0.6-1.4 GB RSS for this small set and up to 37 GB on larger batches of code;
Qwen3-Embedding 53 events/s on Metal, 1.9 GB in Pitwall. The fixed table and names search cost microseconds; the 2B rewrite
took 0.39 s per query on Metal (0.68 s grounded).

## What the numbers say

1. **Identifier queries are solved already.** `paddock-names` in front of BM25 finds misspelt, joined and split names
   (0.97 recall@5); no embedding does as well (0.67-0.80). Embeddings add nothing here.
2. **Plain-language queries are where lexical search fails** (BM25 finds a right event in the top 5 for 60% of them), and
   names search does not change that.
3. **Embeddings do add something for plain-language queries, in proportion to the model's weight.** Model2Vec fused with BM25
   lifts hit@5 from 0.60 to 0.73-0.77 at almost no cost; BGE-small to 0.87; Qwen3-Embedding alone reaches 0.97 and is the only
   method that found the Japanese disk-full event and the Japanese "order received" event from English queries (it is
   multilingual; the others are not).
4. **Query rewriting could be the lightest way to close the gap, but a small model did not do it.** The table, which encodes
   log vocabulary, reaches 0.90 hit@5 with BM25 alone, close to Qwen3 and with no index at all: that is what a good rewriter is
   worth. The real 2B model did not get there. Unguided it made BM25 **worse** (0.50), adding the same generic words
   ("connection refused", "timeout") to almost every query; grounded in the events' own 829 words it still ignored most of
   them (it invented `java.lang.OutOfMemoryError` for "the service crashed") and stayed at BM25's 0.60. Rule 20's "resolve the
   user's words to concrete values" needs a mechanism that is held to the data (choosing from candidate templates or facet
   values, checked against the store), or a larger model; this trial did not measure either.
5. Fusion matters: rewriting + BM25 + a strong embedder is the best plain-language result (0.84 recall@5), but the same fusion
   lowered identifier results, so a router (identifier-like query -> names; plain language -> rewrite or semantic) beats one
   fixed fusion.

## Answer

**Do embeddings add anything over lexical + names search for logs?** For identifier queries, no: names + BM25 is better
than every embedding. For plain-language queries, yes: lexical + names finds a right event in the top 5 for 60% of them, and
nothing cheaper that was actually run did better. The real small-LLM rewrite (unguided or grounded) did not help.

The lightest option that measurably helps is **BM25 + Model2Vec** (potion-code-16M-v2): 0.73 hit@5 (0.77 with
potion-retrieval-32M) for about 3 MB of pure-Rust code, a 32 MB model, milliseconds of start-up and embedding only distinct
message templates (here 505 for 44k lines). That is better than lexical but not "good enough" by itself if the bar is,
say, 0.85 hit@5. Options that clear 0.85: BM25 + BGE-small (0.87, too heavy for an engine component: ONNX Runtime and
gigabytes of RAM, see the Pacenotes ADR) and **Qwen3-Embedding through Pitwall** (0.97, also the only one that bridges English
questions to Japanese events). A rewriter that really knows the log vocabulary would reach about 0.90 with no index (the
table's upper bound), but that rewriter does not exist yet.

So, for Debrief: keep lexical + names as the base. If plain-language search is to work well, the two candidates are (1) a
data-held rewrite through Pitwall that picks from the store's templates and facet values, measured on this set before it is
trusted, and (2) semantic search over message templates, Model2Vec built in and Qwen3-Embedding when Pitwall runs. The second
would change Debrief's "no vector search" decision and is the user's call; this note does not recommend it over trying (1)
first.

## Caveats

- Small: 505 events, 36 queries, generated samples. Real logs have more templates and more noise; treat differences under
  about 0.1 as noise.
- The table rewrite is an upper bound written with knowledge of the samples. Only one rewriting model (2B) and two prompts
  were tried; a larger model, or one that chooses among candidate templates, may do much better and is the next measurement.
- The samples do not contain the exact examples in the question (`401 token expired`, `ENOSPC`, `startup took 14.2 s`,
  `pool exhausted`); the queries test the same kind of gap with the events that exist.
- One run per method, on a shared machine; speed numbers carry the load average.

## Reproduction

The spike lived in `/Volumes/d7sst7/dev-targets/pacenotes-embed-spike` (scripts kept, build output removed):
`scripts/log_templates.py` (events), `scripts/log_queries.py` (labels), `corpus/rewrite.txt` (table),
`scripts/llm_rewrite.py` (LLM rewrite), `spike/eval` (BM25, `paddock-names`, RRF, metrics), `spike/m2v`, `spike/fe` and
`scripts/llama_embed.py` (embedders).
