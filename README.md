# rsbtc — Bitcoin from scratch, in Rust

A from-scratch Bitcoin implementation built for **learning**: practicing Rust and
deepening my understanding of how Bitcoin actually works under the hood. It is a
deliberately simplified model — correctness of understanding matters more here than
production-readiness.

## Origin & approach

This project is based on **[Building Bitcoin in Rust](https://braiins.com/books/building-bitcoin-in-rust)**
by Lukáš Hozda (Braiins). The book is the backbone of the project, but I've adjusted
the implementation in two directions as I go:

- **Closer to real Bitcoin consensus rules** — I cross-reference the actual protocol
  rather than taking the book's simplifications at face value (e.g. UTXO-hash keying,
  miner-fee-based mempool replacement, difficulty clamping).
- **More idiomatic Rust** — the book targets beginner Rust developers, so I rework
  many patterns toward what the wider Rust ecosystem considers idiomatic (newtypes for
  type-safe hashes, blanket trait impls, iterator combinators, `Result`-based error
  handling).

To understand the underlying Bitcoin mechanics beyond the book, I lean on:

- *Mastering Bitcoin* — Andreas Antonopoulos
- *Programming Bitcoin: Learn How to Program Bitcoin from Scratch* — Jimmy Song
- The [Bitcoin BIPs](https://en.bitcoin.it/wiki/Category:BIP)
- [Bitcoin Core documentation](https://bitcoincore.academy/)

This remains a **simplified** implementation. Learning is the goal, not feature parity
with Bitcoin Core.

## About me

I'm a software and data engineer with 15+ years building scalable systems and
large-scale data applications — across healthcare, satellite imagery, cloud
infrastructure, and fintech/banking.

In 2021 I switched to blockchain, working professionally at an Ethereum-based company.
But the more chains I explored, the more Bitcoin stood out — not just for its economics
and philosophy, but because it's *simple*. It doesn't try to be a thousand things. It's
money. Programmable money.

I've been learning and building in the Bitcoin ecosystem since Chaincode Labs' Bitcoin
and Lightning Protocol Development program (2025 cohort). Four intense months — I learned
more than I could digest. Since then I've been going deep, step by step: a 9-month Rust
course, *Mastering Bitcoin*, and a steady diet of BIPs, books, and tutorials.

## License & credit

A learning project. Full credit to Lukáš Hozda and Braiins for
[Building Bitcoin in Rust](https://braiins.com/books/building-bitcoin-in-rust), which
this work builds upon.
