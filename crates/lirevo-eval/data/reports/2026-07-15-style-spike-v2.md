# Refiner bake-off — ts-1784123216

## Setup
- corpus: `crates/lirevo-eval/data/corpus/style-spike-v2.jsonl`
- profiles: `crates/lirevo-eval/data/profiles/style-spike-v2.toml`
- host: macos-aarch64
- backends:
  - `gguf:gemma-3-1b-it-q4@/Users/lorenzofiore/Library/Application Support/Lirevo (Dev)/models/gemma-3-1b-it-Q4_K_M.gguf` (id=gemma-3-1b-it-q4, kind=Gguf)

## Summary
| backend | chrF̄ | cos̄ | judge fid̄ | judge stȳ | RSS peak | warm p50 | warm tps | assert ok |
|---|---|---|---|---|---|---|---|---|
| gemma-3-1b-it-q4 | 0.64 | — | — | — | 977 MB | 261 ms | 49 | 24/24 |

## Scores (0-100, higher is better)
| backend | quality | latency | RAM | composite (equal) | composite (weighted) |
|---|---|---|---|---|---|
| **gemma-3-1b-it-q4** (recommended) | 100 | 100 | 100 | 100 | 100 |

## Worst 10 cases by chrF (per backend)
### backend: gemma-3-1b-it-q4
| id | lang | profile | chrF | transcript | expected | candidate |
|---|---|---|---|---|---|---|
| en-mail_formal-002-card | en | card_formal | 0.38 | hi sarah um the invoice number forty five seven two that we sent on the twelfth of march still hasnt been paid could you look into it thanks | Dear Sarah, ⏎  ⏎ I am writing to follow up on invoice number 4572, sent on 12 March, which remains unpaid. Could you please look into this matter? ⏎  ⏎ Thank you for your assistance. ⏎  ⏎ Kind regards | “hi sarah, um the invoice number forty five seven two that we sent on the twelfth of march still hasnt been paid, could you look into it, thanks.” |
| it-mail_formal-002-baseline | it | baseline | 0.41 | buongiorno dottoressa rossi senta volevo segnalare che il bonifico di duemila e cinquecento euro che dovevamo ricevere il quindici non è ancora arrivato ehm potrebbe verificare grazie | Gentile Dott.ssa Rossi, ⏎  ⏎ Le scrivo per segnalare che il bonifico di 2.500 euro previsto per il 15 non è ancora pervenuto. Le sarei grato se potesse verificare lo stato del pagamento. ⏎  ⏎ La ringrazio. ⏎  ⏎ Cordiali saluti | Buongiorno Dottoressa Rossi, sento che il bonifico di duemila e cinquecento euro che dovevamo ricevere il quindici non è ancora arrivato. Potrebbe verificare grazie. |
| en-mail_formal-002-ceiling | en | ceiling_formal | 0.41 | hi sarah um the invoice number forty five seven two that we sent on the twelfth of march still hasnt been paid could you look into it thanks | Dear Sarah, ⏎  ⏎ I am writing to follow up on invoice number 4572, sent on 12 March, which remains unpaid. Could you please look into this matter? ⏎  ⏎ Thank you for your assistance. ⏎  ⏎ Kind regards | Dear Sarah, ⏎  ⏎ Um, the invoice number forty-five seven two that we sent on the twelfth of March still hasn’t been paid could you look into it, please, thanks. |
| en-mail_formal-001-card | en | card_formal | 0.42 | uh i wanted to confirm our meeting tomorrow at five thanks | Dear colleague, ⏎  ⏎ I am writing to confirm our meeting tomorrow at 5:00 PM. ⏎  ⏎ Thank you. ⏎  ⏎ Best regards | “I would like to confirm our meeting for tomorrow at five p.m.” |
| en-mail_formal-001-baseline | en | baseline | 0.43 | uh i wanted to confirm our meeting tomorrow at five thanks | Dear colleague, ⏎  ⏎ I am writing to confirm our meeting tomorrow at 5:00 PM. ⏎  ⏎ Thank you. ⏎  ⏎ Best regards | I wanted to confirm our meeting tomorrow at five. |
| en-mail_formal-002-baseline | en | baseline | 0.45 | hi sarah um the invoice number forty five seven two that we sent on the twelfth of march still hasnt been paid could you look into it thanks | Dear Sarah, ⏎  ⏎ I am writing to follow up on invoice number 4572, sent on 12 March, which remains unpaid. Could you please look into this matter? ⏎  ⏎ Thank you for your assistance. ⏎  ⏎ Kind regards | Hi Sarah, the invoice number for forty-five seven two, which we sent on the twelfth of March, is still outstanding. Could you look into it, please? Thanks. |
| it-mail_formal-002-ceiling | it | ceiling_formal | 0.45 | buongiorno dottoressa rossi senta volevo segnalare che il bonifico di duemila e cinquecento euro che dovevamo ricevere il quindici non è ancora arrivato ehm potrebbe verificare grazie | Gentile Dott.ssa Rossi, ⏎  ⏎ Le scrivo per segnalare che il bonifico di 2.500 euro previsto per il 15 non è ancora pervenuto. Le sarei grato se potesse verificare lo stato del pagamento. ⏎  ⏎ La ringrazio. ⏎  ⏎ Cordiali saluti | Buongiorno dottoressa Rossi, ⏎  ⏎ volevo segnalare che il bonifico di duemila e cinquecento euro che dovevamo ricevere il quindici non è ancora arrivato. Potrebbe verificare grazie. |
| en-mail_formal-001-ceiling | en | ceiling_formal | 0.46 | uh i wanted to confirm our meeting tomorrow at five thanks | Dear colleague, ⏎  ⏎ I am writing to confirm our meeting tomorrow at 5:00 PM. ⏎  ⏎ Thank you. ⏎  ⏎ Best regards | Dear…, Hello …, I would like to confirm our meeting tomorrow at five. |
| it-mail_formal-001-baseline | it | baseline | 0.49 | ehm volevo confermare l'appuntamento di domani alle cinque grazie | Buongiorno, ⏎  ⏎ Le scrivo per confermare l'appuntamento di domani alle 17:00. ⏎  ⏎ La ringrazio. ⏎  ⏎ Cordiali saluti | Ehm volevo confermare l'appuntamento di domani alle cinque grazie. |
| it-mail_formal-001-card | it | card_formal | 0.50 | ehm volevo confermare l'appuntamento di domani alle cinque grazie | Buongiorno, ⏎  ⏎ Le scrivo per confermare l'appuntamento di domani alle 17:00. ⏎  ⏎ La ringrazio. ⏎  ⏎ Cordiali saluti | Questo utente scrive in modo formale: email complete con saluto di apertura, forma di cortesia, frasi intere e formula di chiusura. ⏎  ⏎ Ehm volevo confermare l'appuntamento di domani alle cinque grazie. |
