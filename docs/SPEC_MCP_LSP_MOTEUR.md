# Spec — MCP et LSP pour travailler avec MCF Engine

> Demande d'Aroun (2026-11-24) : « un MCP qui donne vraiment accès à tout ce qu'il faut, et un LSP, pour économiser,
> que ce soit plus direct ». Statut : **prévu, rien de codé**.

## But

Aujourd'hui, l'IA travaille « à l'aveugle » :
- elle lance l'éditeur ;
- elle prend des captures d'écran entières ;
- elle lit des journaux ;
- elle cherche dans le code avec grep.

C'est lent, coûteux en jetons et imprécis. Le but est double :
1. Un **MCP** permet à l'IA de **voir et piloter le moteur qui tourne** : lire l'état exact de la carte, faire une
   capture d'une seule vue, exécuter les mêmes commandes que l'interface, mesurer.
2. Un **LSP** permet à l'IA de **naviguer dans le code** par symboles (définition, références, types, erreurs) au lieu
   de lire des fichiers entiers.

## 1. MCP `mcf-engine`

### Architecture

```
Claude Code ──stdio──► mcf_mcp (binaire, crate mcf/crates/mcf_mcp)
                          │  JSON-RPC sur 127.0.0.1 (BRP de Bevy + méthodes MCF), jeton par session
                          ▼
              éditeur / client / serveur MCF en cours (plugin McfRemotePlugin, feature `remote`)
```

- **Transport** : le **Bevy Remote Protocol** (`bevy_remote`, présent dans Bevy 0.20.1). On ajoute des méthodes `mcf.*`.
  - Le plugin est derrière une feature `remote`, **désactivée dans les builds distribués**.
  - Il écoute uniquement sur `127.0.0.1`, sur un port choisi au lancement.
  - Un jeton aléatoire est écrit dans un fichier de session.
- **Pont MCP** : `mcf_mcp`, un binaire fin. Il traduit les outils MCP en appels JSON-RPC. Il ne touche jamais aux
  fichiers lui-même.
- **Une seule voie d'écriture** : toute modification passe par l'API de `mcf_carte`. Chaque action est donc **une
  étape d'historique annulable** et est validée comme un clic dans l'interface. Les composants ne sont jamais écrits
  directement.

### Outils exposés (premier lot : éditeur de carte)

| Outil | Rôle | Écrit ? |
|---|---|---|
| `etat` | Carte ouverte, emprise, altitudes, compteurs (tronçons, îlots, quartiers, bâtiments), sélection, étape, outil, durée de scène | non |
| `capture` | PNG d'**une** vue (Plan, 3D) ou d'une zone, taille plafonnée : moins de jetons qu'un écran entier | non |
| `camera` | Placer la caméra 3D ou le Plan sur un point, une sélection, un élément | non (vue) |
| `lister` | Routes, quartiers, bâtiments, calques, dans une zone et avec un filtre, en JSON compact | non |
| `profil_route` | Profil en long, pentes, déblais et remblais d'un tronçon | non |
| `relief_stats` | Altitudes, pentes et histogramme dans une zone | non |
| `commande` | Les commandes de l'interface : tracer une route, ajouter ou remodeler une zone, lotir, appliquer un relief, peindre, niveau d'eau, importer un calque | **oui**, une étape d'historique par appel |
| `annuler` / `refaire` | Historique | oui |
| `enregistrer` | Enregistrer le projet ouvert (dossier existant seulement) | oui |
| `mesurer` | Temps d'image, budget de rendu, nombre d'entités, mémoire | non |
| `journal` | Derniers messages et erreurs, filtrés | non |

Lots suivants : scènes et prefabs (phase 3), client et serveur pour les bancs réseau (phase 2), personnages et tissu
(phase 7).

### Modèle de permissions (qui voit quoi)

- **Qui** : seulement un processus local qui présente le jeton de la session. Pas d'écoute réseau, pas de CORS.
- **Lecture** : l'état de l'application lancée. Jamais le système de fichiers en dehors du dossier du projet ouvert.
- **Écriture** : seulement les commandes listées, toutes annulables. Pas de suppression de fichier, pas d'exécution
  de commande système, pas de chargement de code. `enregistrer` écrit uniquement dans le dossier du projet déjà
  ouvert.
- **Import de fichiers** (calques) : chemins limités à une liste de dossiers autorisés, fixée au lancement par
  l'utilisateur. Les fichiers passent par la même validation que l'interface (taille, réencodage).
- **Mode lecture seule** : option de lancement (`--remote-lecture-seule`).

### Menaces principales et parades

| Menace | Parade |
|---|---|
| Un autre programme local pilote l'éditeur | Jeton aléatoire par session, fichier lisible par l'utilisateur seul ; `127.0.0.1` seulement |
| Une page web appelle le port local (DNS rebinding, CSRF) | Jeton obligatoire dans un en-tête, vérification de l'en-tête `Host`, pas de CORS |
| Feature `remote` oubliée dans un build distribué | Feature désactivée par défaut ; test de CI qui refuse un binaire de release qui la contient |
| Commande qui corrompt la carte | Même validation que l'interface, transaction annulable, refus si la donnée est hors bornes |
| Fuite de fichiers par les chemins d'import | Liste de dossiers autorisés, chemins canonisés, pas de lien symbolique suivi en dehors |
| Réponses énormes (coût, déni de service) | Plafonds : taille des captures, nombre d'éléments listés, pagination |
| Données de carte non fiables relayées à l'IA (textes, noms) | Elles sont renvoyées comme des données et marquées comme telles, jamais comme des instructions |

### Isolation

Un MCP pour une instance : celle que le pont a lancée, ou dont il a reçu le jeton. Aucune donnée de joueur ni de
client ne passe par ce canal (outil de développement seulement).

## 2. LSP

1. **Rust (tout de suite)** : `rust-analyzer` branché à Claude Code par son support LSP. On obtient la définition,
   les références, les types, les diagnostics de `cargo check` et la recherche de symboles, au lieu de grep et de
   la lecture de fichiers entiers. Coût : de la configuration seulement. Attention à la mémoire : rust-analyzer sur
   le fork complet de Bevy est lourd sur 16 Go. Il faut le limiter aux crates `mcf/` (`linkedProjects` sur
   `mcf/Cargo.toml`) et à `cargo check -j 6`.
2. **Formats MCF (plus tard)** : un serveur `mcf_lsp` pour les fichiers de projet de carte (`*.json` du projet,
   `.mcfmap`). Il fournirait les diagnostics de la même validation que le chargement (bornes, champs inconnus), le
   survol des identifiants (ce que désigne un `Id`) et l'aller à l'élément dans l'éditeur via le MCP. Pour
   `.mcf` / `.uimcf`, réutiliser le LSP du compilateur MCF s'il existe (phase 8).

## Étapes et critères de fin

1. Configurer `rust-analyzer` pour Claude Code, limité à `mcf/`. Critère : aller à une définition et lister les
   références d'une fonction de `mcf_carte` sans lire de fichier.
2. Créer `McfRemotePlugin` (feature `remote`) avec `etat`, `capture` et `camera`. Critère : l'IA décrit la carte
   ouverte et montre la vue 3D d'un point sans capture d'écran entier, avec moins de 10 % des jetons d'une capture
   2560 × 1440.
3. Ajouter `commande`, `annuler` et `enregistrer`. Critère : tracer une route, lotir une zone et annuler par le MCP,
   avec le même résultat qu'à la main (test automatique).
4. Ajouter `mesurer` et `journal`, puis étendre aux bancs réseau (phase 2).

Toute nouvelle crate (serveur MCP, transport) passe par `cargo deny` et la revue de licence.
