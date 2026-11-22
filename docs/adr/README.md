# Architecture Decision Records (ADR)

Ce dossier conserve les décisions techniques structurantes de MCF Engine.
Une ADR ne remplace pas un prototype ni une mesure : elle relie la décision aux
preuves disponibles et rend ses conséquences explicites.

## Règles

- Créer une ADR avant d'intégrer une dépendance ou de modifier une architecture
  durable (réseau, physique, streaming, format, scripting, persistance).
- Numéroter les fichiers dans l'ordre : `ADR-0001-sujet-court.md`.
- Ne marquer une décision comme acceptée qu'après comparaison, prototype et
  vérification de compatibilité Bevy/Rust, licence et maintenance.
- Réviser une décision par une nouvelle ADR qui la remplace ; ne pas réécrire
  l'historique.

Le modèle à copier est [ADR-0000-template.md](ADR-0000-template.md).
