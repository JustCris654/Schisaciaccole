---
status: accepted
updated: 2026-11-14
---

# Decisioni: preferenze dei meeting

| Data | Decisione |
| --- | --- |
| 2026-11-14 | I gruppi restano esplosi dal server nella lista. Una bozza creata con una lista completa non viene unita a una lista incompleta, per non perdere le disattivazioni dei membri se l'espansione fallisce per poco. |
| 2026-11-14 | La bozza segue Calendar a ogni ricarica della lista: nuovi invitati attivi, invitati tolti fuori, modifiche dell'utente conservate. |
| 2026-11-14 | Il conteggio dei partecipanti sulla riga della lista conta le persone che **Avvia** userebbe. |
| 2026-11-14 | Righe dell'editor: invitati per nome, poi persone aggiunte nell'ordine salvato. |
| 2026-11-14 | Nell'editor il pulsante diventa "Avvia Daily" quando l'interruttore della riga è acceso. |
| 2026-11-14 | L'interruttore Daily resta solo sulla riga della lista; **Avvia** dall'editor lo segue senza mostrarlo. |
| 2026-11-14 | **Elimina preferenza** resta. Salvare senza aggiunte né rinomine elimina la preferenza esistente. |
| 2026-11-14 | La migrazione cancella tutti i template esistenti invece di convertirli. |
| 2026-11-14 | Rotte nuove `PUT /api/v1/preferences` e `DELETE /api/v1/preferences/{id}`; tolte `/plan` e `/templates`. La preferenza arriva dentro ogni `Meeting`. App e server si rilasciano insieme. |
| 2026-11-14 | Tutte le righe dell'editor hanno la casella; disattivare è sempre temporaneo. **×** toglie solo le persone aggiunte. |
| 2026-11-14 | Le persone aggiunte salvate valgono anche per **Avvia** dalla lista (Daily e turni). |
| 2026-11-14 | Le modifiche non salvate restano in una bozza per evento finché l'app è aperta. |
| 2026-11-14 | La preferenza salva le persone aggiunte e le rinomine degli invitati, non le esclusioni né i pesi. |
