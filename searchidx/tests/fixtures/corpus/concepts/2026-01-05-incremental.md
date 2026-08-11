---
type: concept
tags: [search, architecture]
---
# 增量索引设计

本节讲增量索引的设计,全量重建代价随语料增长而升高,所以我们引入了增量策略,
只重新索引发生变化的文件,而不是每次都做 full rebuild。
