# Refiner bake-off — ts-1784127723

## Setup
- corpus: `crates/lirevo-eval/data/corpus/style-spike-v5.jsonl`
- profiles: `crates/lirevo-eval/data/profiles/style-spike-v5.toml`
- host: macos-aarch64
- backends:
  - `gguf:gemma-3-1b-it-q4@/Users/lorenzofiore/Library/Application Support/Lirevo (Dev)/models/gemma-3-1b-it-Q4_K_M.gguf` (id=gemma-3-1b-it-q4, kind=Gguf)

## Summary
| backend | chrF̄ | cos̄ | judge fid̄ | judge stȳ | RSS peak | warm p50 | warm tps | assert ok |
|---|---|---|---|---|---|---|---|---|
| gemma-3-1b-it-q4 | 0.59 | — | — | — | 962 MB | 268 ms | 47 | 16/16 |

## Scores (0-100, higher is better)
| backend | quality | latency | RAM | composite (equal) | composite (weighted) |
|---|---|---|---|---|---|
| **gemma-3-1b-it-q4** (recommended) | 100 | 100 | 100 | 100 | 100 |

## Worst 10 cases by chrF (per backend)
### backend: gemma-3-1b-it-q4
| id | lang | profile | chrF | transcript | expected | candidate |
|---|---|---|---|---|---|---|
| en-chat_casual-001-fewshot | en | fewshot_casual | 0.16 | uh yeah see you tomorrow at eight at the usual place | see you tomorrow at 8 at the usual place | no honestly i can't make it today, sorry |
| en-mail_formal-001-baseline | en | baseline | 0.43 | uh i wanted to confirm our meeting tomorrow at five thanks | Dear colleague, ⏎  ⏎ I am writing to confirm our meeting tomorrow at 5:00 PM. ⏎  ⏎ Thank you. ⏎  ⏎ Best regards | I wanted to confirm our meeting tomorrow at five. |
| en-chat_casual-002-fewshot | en | fewshot_casual | 0.43 | omg wait did he really say that i cant believe it like after everything | omg wait did he really say that?? i can't believe it, after everything | no honestly i can't make it today, like after everything |
| en-mail_formal-002-baseline | en | baseline | 0.45 | hi sarah um the invoice number forty five seven two that we sent on the twelfth of march still hasnt been paid could you look into it thanks | Dear Sarah, ⏎  ⏎ I am writing to follow up on invoice number 4572, sent on 12 March, which remains unpaid. Could you please look into this matter? ⏎  ⏎ Thank you for your assistance. ⏎  ⏎ Kind regards | Hi Sarah, the invoice number for forty-five seven two, which we sent on the twelfth of March, is still outstanding. Could you look into it, please? Thanks. |
| it-mail_formal-002-fewshot | it | fewshot_formal | 0.45 | buongiorno dottoressa rossi senta volevo segnalare che il bonifico di duemila e cinquecento euro che dovevamo ricevere il quindici non è ancora arrivato ehm potrebbe verificare grazie | Gentile Dott.ssa Rossi, ⏎  ⏎ Le scrivo per segnalare che il bonifico di 2.500 euro previsto per il 15 non è ancora pervenuto. Le sarei grato se potesse verificare lo stato del pagamento. ⏎  ⏎ La ringrazio. ⏎  ⏎ Cordiali saluti | Buongiorno dottoressa Rossi, ⏎  ⏎ sentevo volevo segnalare che il bonifico di duemila e cinquecento euro che dovevamo ricevere il quindici non è ancora arrivato: potrebbe verificare grazie. |
| it-mail_formal-002-baseline | it | baseline | 0.46 | buongiorno dottoressa rossi senta volevo segnalare che il bonifico di duemila e cinquecento euro che dovevamo ricevere il quindici non è ancora arrivato ehm potrebbe verificare grazie | Gentile Dott.ssa Rossi, ⏎  ⏎ Le scrivo per segnalare che il bonifico di 2.500 euro previsto per il 15 non è ancora pervenuto. Le sarei grato se potesse verificare lo stato del pagamento. ⏎  ⏎ La ringrazio. ⏎  ⏎ Cordiali saluti | Buongiorno dottoressa Rossi, senta, volevo segnalare che il bonifico di duemila e cinquecento euro che dovevamo ricevere il quindici non è ancora arrivato. Potrebbe verificare grazie. |
| it-mail_formal-001-baseline | it | baseline | 0.49 | ehm volevo confermare l'appuntamento di domani alle cinque grazie | Buongiorno, ⏎  ⏎ Le scrivo per confermare l'appuntamento di domani alle 17:00. ⏎  ⏎ La ringrazio. ⏎  ⏎ Cordiali saluti | Ehm volevo confermare l'appuntamento di domani alle cinque grazie. |
| it-mail_formal-001-fewshot | it | fewshot_formal | 0.52 | ehm volevo confermare l'appuntamento di domani alle cinque grazie | Buongiorno, ⏎  ⏎ Le scrivo per confermare l'appuntamento di domani alle 17:00. ⏎  ⏎ La ringrazio. ⏎  ⏎ Cordiali saluti | Buongiorno, ⏎  ⏎ Le confermo l'appuntamento di domani alle cinque. |
| en-mail_formal-002-fewshot | en | fewshot_formal | 0.55 | hi sarah um the invoice number forty five seven two that we sent on the twelfth of march still hasnt been paid could you look into it thanks | Dear Sarah, ⏎  ⏎ I am writing to follow up on invoice number 4572, sent on 12 March, which remains unpaid. Could you please look into this matter? ⏎  ⏎ Thank you for your assistance. ⏎  ⏎ Kind regards | Dear Sarah, ⏎  ⏎ The invoice number forty-five seven two, which we sent on the twelfth of March, is still unpaid. Could you please look into it? ⏎  ⏎ Thank you, ⏎  ⏎ Best regards, |
| en-mail_formal-001-fewshot | en | fewshot_formal | 0.58 | uh i wanted to confirm our meeting tomorrow at five thanks | Dear colleague, ⏎  ⏎ I am writing to confirm our meeting tomorrow at 5:00 PM. ⏎  ⏎ Thank you. ⏎  ⏎ Best regards | Dear all, ⏎  ⏎ I wanted to confirm our meeting tomorrow at five. ⏎  ⏎ Best regards, |
