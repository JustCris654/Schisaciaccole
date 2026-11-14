# Design e decisioni

Questa cartella raccoglie i design e le decisioni di sviluppo di Schisaciaccole. È scritta per chi sviluppa il progetto, non per chi lo usa: la guida è in [`../guide/`](../guide/).

## Argomenti

| Argomento | File | Stato | Aggiornato |
| --- | --- | --- | --- |
| _Nessun argomento per ora_ | | | |

## Convenzioni

Ogni file inizia con un frontmatter YAML:

```yaml
---
status: proposed | accepted | superseded | rejected
updated: YYYY-MM-DD
alternative-to: ...   # opzionale
superseded-by: ...    # opzionale
---
```

- `status`: `proposed` è una proposta in valutazione, `accepted` è approvata e in vigore, `superseded` è sostituita da un altro documento (indicato in `superseded-by`), `rejected` è scartata.
- `updated`: data del contenuto, non del commit.
- Gli agenti trattano come vincolante solo ciò che è `accepted`; ciò che è `proposed` è contesto.

Ogni argomento ha una cartella con:

- `design.md`: il design corrente;
- `decisions.md`: il registro datato delle decisioni, la più recente in alto, una riga per decisione;
- `open-questions.md`: una checklist. Una domanda decisa si spunta con la risposta in linea e la data, non si cancella;
- `milestones.md`: solo quando c'è un piano.

Le decisioni grandi e contese possono avere un ADR completo in `docs/design/adr/NNNN-titolo.md`.

Un nuovo argomento va aggiunto alla tabella qui sopra e all'indice in [`../README.md`](../README.md).
