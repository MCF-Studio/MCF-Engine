# Inventaire des dépendances étudiées

Ce document est un registre de décision, pas une autorisation d'ajouter une
crate. Une ligne `à prototyper` implique encore : version compatible, licence,
maintenance, prototype isolé et ADR si l'intégration structure l'architecture.
Le contrôle applicable avant une modification est détaillé dans le
[garde-fou des dépendances et formats](GARDE_FOU_DEPENDANCES_ET_FORMATS.md).

| Domaine | Projet | Statut | Justification / prochaine preuve |
|---|---|---|---|
| Moteur | Bevy 0.20.1 (fork local) | référence technique | Socle actuel ; synchronisation amont à préserver. |
| UI éditeur | bevy_egui 0.43.1 | adopté pour la base | Versions compatibles avec Bevy 0.20 et egui 0.36 ; maintenir les contrôles de compilation. |
| UI éditeur | egui_dock 0.21.x | adopté, intégration à terminer | Dépendance disponible ; panneaux dockables et persistance de layout à mettre en œuvre. |
| Réseau | Lightyear | à prototyper | Comparaison obligatoire contre Replicon + Renet sur les paliers 32→256 bots. |
| Réseau | bevy_replicon + Renet | à prototyper | Même protocole, mêmes métriques et conditions que Lightyear. |
| Physique | Avian | à prototyper | Personnage, pentes, véhicules, headless et coût réseau. |
| Physique | Rapier | à prototyper | Même scène et mêmes critères qu'Avian. |
| Scripting | `.mcf` / `.uimcf` | garé | Décision explicitement différée par la roadmap ; aucune runtime à intégrer. |
| Assets | glTF / KTX2-Basis | référence technique | À valider dans un pipeline import avec métadonnées, LOD et collisions. |
| Profiling | outils Bevy + traces | référence technique | Compléter par export brut des bancs A et B avant choix de solutions externes. |

## Mise à jour d'une ligne

Indiquer la version exacte, la licence vérifiée, l'URL/source, la date de
consultation, le commit du prototype, les mesures et le lien ADR. Ne pas
confondre popularité, compatibilité et résultat mesuré.
