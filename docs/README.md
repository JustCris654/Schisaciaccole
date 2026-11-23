# Documentazione Schisaciaccole

La documentazione è divisa in due parti:

- [`guide/`](./guide/): come funziona e come si usa Schisaciaccole, per chi lavora sul progetto.
- [`design/`](./design/): design e decisioni di sviluppo, con il loro stato ([convenzioni](./design/README.md)).

Presentazione, funzionalità, scorciatoie e installazione sono descritti nel [README](../README.md) alla radice del repository.

Quando codice e documentazione divergono, vale il codice.

## Guida

- [Setup del backend](./guide/setup-backend.md): Google Cloud Console, configurazione, avvio del server e build dell'app
- [Panoramica](./guide/panoramica.md): cosa è Schisaciaccole, le due modalità di funzionamento, struttura del repo
- [Schermate e flusso frontend-backend](./guide/flusso-frontend-backend.md): tutte le schermate dell'app desktop e le chiamate al server per login, meeting e preferenze dei meeting ([versione HTML](./flusso-frontend-backend.html))

## Design

- [Preferenze dei meeting senza pesi, editor locale](./design/preferenze-meeting/design.md) (accepted): dati tutti dalla lista, editor locale con bozza per evento, preferenze senza pesi
- [Temi selezionabili: quattro temi, chiaro e scuro, predefinito di build](./design/temi/design.md) (accepted): catalogo guidato dai dati, tre temi OpenMove, scelta locale e predefinito a compilazione

Le convenzioni sono in [`design/README.md`](./design/README.md).
