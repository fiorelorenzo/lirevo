# Refiner bake-off — ts-1784122491

## Setup
- corpus: `crates/lirevo-eval/data/corpus/style-spike.jsonl`
- profiles: `crates/lirevo-eval/data/profiles/style-spike.toml`
- host: macos-aarch64
- backends:
  - `gguf:gemma-3-1b-it-q4@/Users/lorenzofiore/Library/Application Support/Lirevo (Dev)/models/gemma-3-1b-it-Q4_K_M.gguf` (id=gemma-3-1b-it-q4, kind=Gguf)

## Summary
| backend | chrF̄ | cos̄ | judge fid̄ | judge stȳ | RSS peak | warm p50 | warm tps | assert ok |
|---|---|---|---|---|---|---|---|---|
| gemma-3-1b-it-q4 | 0.65 | — | — | — | 976 MB | 183 ms | 61 | 24/24 |

## Scores (0-100, higher is better)
| backend | quality | latency | RAM | composite (equal) | composite (weighted) |
|---|---|---|---|---|---|
| **gemma-3-1b-it-q4** (recommended) | 100 | 100 | 100 | 100 | 100 |

## Worst 10 cases by chrF (per backend)
### backend: gemma-3-1b-it-q4
| id | lang | profile | chrF | transcript | expected | candidate |
|---|---|---|---|---|---|---|
| en-mail_formal-002-card | en | card_formal | 0.37 | hi sarah um the invoice number forty five seven two that we sent on the twelfth of march still hasnt been paid could you look into it thanks | Dear Sarah, ⏎  ⏎ I am writing to follow up on invoice number 4572, sent on 12 March, which remains unpaid. Could you please look into this matter? ⏎  ⏎ Thank you for your assistance. ⏎  ⏎ Kind regards | “Sarah, the invoice number forty-five seven two that we sent on the twelfth of March still hasn’t been paid, could you look into it, thanks.” |
| en-mail_formal-002-baseline | en | baseline | 0.37 | hi sarah um the invoice number forty five seven two that we sent on the twelfth of march still hasnt been paid could you look into it thanks | Dear Sarah, ⏎  ⏎ I am writing to follow up on invoice number 4572, sent on 12 March, which remains unpaid. Could you please look into this matter? ⏎  ⏎ Thank you for your assistance. ⏎  ⏎ Kind regards | hi sarah um the invoice number forty five seven two that we sent on the twelfth of march still hasnt been paid could you look into it thanks |
| en-mail_formal-001-card | en | card_formal | 0.42 | uh i wanted to confirm our meeting tomorrow at five thanks | Dear colleague, ⏎  ⏎ I am writing to confirm our meeting tomorrow at 5:00 PM. ⏎  ⏎ Thank you. ⏎  ⏎ Best regards | I'd like to confirm our meeting tomorrow at five. |
| en-mail_formal-001-baseline | en | baseline | 0.43 | uh i wanted to confirm our meeting tomorrow at five thanks | Dear colleague, ⏎  ⏎ I am writing to confirm our meeting tomorrow at 5:00 PM. ⏎  ⏎ Thank you. ⏎  ⏎ Best regards | I wanted to confirm our meeting tomorrow at five. |
| en-mail_formal-001-ceiling | en | ceiling_formal | 0.43 | uh i wanted to confirm our meeting tomorrow at five thanks | Dear colleague, ⏎  ⏎ I am writing to confirm our meeting tomorrow at 5:00 PM. ⏎  ⏎ Thank you. ⏎  ⏎ Best regards | Dear Ms. Johnson, ⏎  ⏎ I hope this email finds you well. ⏎  ⏎ I am writing to formally confirm our scheduled meeting for tomorrow, [Date], at 5:00 PM. I anticipate we will be discussing [briefly state the topic of the meeting]. ⏎  ⏎ Please let me know if there are any adjustments to this time that may be necessary. I appreciate your prompt attention to this matter. ⏎  ⏎ Best regards, ⏎  ⏎ [Your Name] ⏎  ⏎ Kind regards, ⏎  ⏎ [Your Name] ⏎  ⏎ Sincerely, ⏎  ⏎ [Your Name] |
| it-mail_formal-002-baseline | it | baseline | 0.46 | buongiorno dottoressa rossi senta volevo segnalare che il bonifico di duemila e cinquecento euro che dovevamo ricevere il quindici non è ancora arrivato ehm potrebbe verificare grazie | Gentile Dott.ssa Rossi, ⏎  ⏎ Le scrivo per segnalare che il bonifico di 2.500 euro previsto per il 15 non è ancora pervenuto. Le sarei grato se potesse verificare lo stato del pagamento. ⏎  ⏎ La ringrazio. ⏎  ⏎ Cordiali saluti | Buongiorno Dottoressa Rossi, senta volevo segnalare che il bonifico di duemila e cinquecento euro che dovevamo ricevere il quindici non è ancora arrivato. Potrebbe verificare grazie. |
| it-mail_formal-002-card | it | card_formal | 0.46 | buongiorno dottoressa rossi senta volevo segnalare che il bonifico di duemila e cinquecento euro che dovevamo ricevere il quindici non è ancora arrivato ehm potrebbe verificare grazie | Gentile Dott.ssa Rossi, ⏎  ⏎ Le scrivo per segnalare che il bonifico di 2.500 euro previsto per il 15 non è ancora pervenuto. Le sarei grato se potesse verificare lo stato del pagamento. ⏎  ⏎ La ringrazio. ⏎  ⏎ Cordiali saluti | Buongiorno dottoressa Rossi, ⏎  ⏎ sentevo che volevo segnalare che il bonifico di duemila e cinquecento euro che dovravamo ricevere il quindici non è ancora arrivato. Potrebbe verificare grazie. |
| it-mail_formal-001-baseline | it | baseline | 0.46 | ehm volevo confermare l'appuntamento di domani alle cinque grazie | Buongiorno, ⏎  ⏎ Le scrivo per confermare l'appuntamento di domani alle 17:00. ⏎  ⏎ La ringrazio. ⏎  ⏎ Cordiali saluti | Volevo confermare l'appuntamento di domani alle cinque |
| it-mail_formal-001-card | it | card_formal | 0.46 | ehm volevo confermare l'appuntamento di domani alle cinque grazie | Buongiorno, ⏎  ⏎ Le scrivo per confermare l'appuntamento di domani alle 17:00. ⏎  ⏎ La ringrazio. ⏎  ⏎ Cordiali saluti | Volevo confermare l'appuntamento di domani alle cinque. |
| en-mail_formal-002-ceiling | en | ceiling_formal | 0.49 | hi sarah um the invoice number forty five seven two that we sent on the twelfth of march still hasnt been paid could you look into it thanks | Dear Sarah, ⏎  ⏎ I am writing to follow up on invoice number 4572, sent on 12 March, which remains unpaid. Could you please look into this matter? ⏎  ⏎ Thank you for your assistance. ⏎  ⏎ Kind regards | Dear Sarah, ⏎  ⏎ I trust this email finds you well. ⏎  ⏎ I am writing to inquire about the status of invoice number forty-five seven two, which was sent on the twelfth of March.  I have yet to receive payment for this invoice. ⏎  ⏎ Could you please investigate this matter and determine the current status of the outstanding balance? ⏎  ⏎ At 5:00 PM today, I would appreciate it if you could provide an update as to when I might expect payment. ⏎  ⏎ Best regards, ⏎  ⏎ Kind regards, ⏎  ⏎ [Your Name] |
