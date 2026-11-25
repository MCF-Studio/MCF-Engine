# Spec — MCP et LSP pour travailler avec MCF Engine

> Demande d'Aroun (2026-11-24) : « un MCP qui donne vraiment accès à tout ce qu'il faut, et un LSP, pour économiser,
> que ce soit plus direct ». Statut (2026-11-25) : **étapes 2 et 3 faites**, étape 1 configurée (à vérifier), étape 4
> à faire. Voir « État » en fin de document.

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
                          │  JSON-RPC ligne à ligne sur 127.0.0.1 (méthodes mcf.*), jeton par session
                          ▼
              éditeur / client / serveur MCF en cours (module `distant`, feature `remote`, option `--remote`)
```

- **Transport** : JSON-RPC à nous (une ligne de requête, une ligne de réponse, une connexion par appel), méthodes
  `mcf.*` en liste fermée. **Pas** le transport HTTP de `bevy_remote` : ses méthodes ne voient pas les en-têtes (ni
  jeton ni contrôle de `Host` possibles) et il expose l'écriture brute des composants (`world.insert_components`…),
  contraire à « une seule voie d'écriture ». Sans HTTP, une page web qui vise le port envoie `POST / HTTP/1.1`, refusé.
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

## État (2026-11-25)

- **Étape 1** : rust-analyzer installé (`rustup component add rust-analyzer`). Le fichier `rust-analyzer.toml` ne
  suffit pas : `linkedProjects` n'y est pas lu, rust-analyzer chargeait le workspace Bevy de la racine (2,7 Go, aucune
  référence trouvée dans `mcf/`). Remplacé par le plugin `mcf-rust-lsp` (`mcf/tools/claude/`), activé dans
  `.claude/settings.json` à la place du plugin officiel : `workspaceFolder` = `mcf/`, 2 fils, pas de `cargo check` à
  l'enregistrement. **À vérifier** après redémarrage de Claude Code (critère de fin non encore prouvé).
- **Étape 2** : fait.
  - Éditeur : `mcf/crates/mcf_editor/src/conception/distant.rs`, lancement `cargo run -p mcf_editor --features
    dev,remote -- --remote`. Fichier de session `%LOCALAPPDATA%\MCF-Engine\remote\editeur.json` (port et jeton de
    256 bits), effacé à la fermeture.
  - Pont : crate `mcf_mcp` (serde_json seulement), déclarée dans `.mcp.json` (binaire `mcf/target/debug/mcf_mcp.exe`,
    à construire avec `cargo build -p mcf_mcp`).
  - Essai réel : `etat` décrit la carte de test (5 tronçons, 11 bâtiments, 4 zones libres, image 6,8 ms, scène
    158 ms). `capture` 3D renvoie 417 × 768 px (≈ 430 jetons d'image contre ≈ 4 900 pour 2560 × 1440, soit 9 %),
    Plan 278 × 512 px. Critère de l'étape 2 atteint, de justesse : rester à 768 px ou moins par défaut.
  - Tests : refus d'une requête HTTP, d'un mauvais jeton, d'une méthode hors liste ; relais d'une requête valide ;
    base64 ; réduction d'image ; protocole MCP du pont.
- **Étape 3** : faite. Outils `lister` (tronçons, quartiers, bâtiments, lignes ; 200 au plus par catégorie) et
  `commande` : `tracer_route`, `ligne_electrique`, `affecter`, `zone_libre`, `lotir`, `poser_modele`, `transformer`,
  `relief`, `nommer`, `annuler`, `refaire`, `enregistrer` (dossier déjà ouvert seulement). Chaque appel = une étape
  d'historique ; refus avec `--remote-lecture-seule`. Test automatique `commandes_comme_a_la_main_et_annulables` :
  même réseau, même quartier, même bâti qu'à la main ; annuler / refaire ; entrées hors bornes refusées sans effet.
  Le lotissement dépend des identifiants des tronçons (tirés au hasard à chaque tracé, à la main aussi) : deux
  tracés identiques ne donnent pas le même bâti, comportement existant et non propre au MCP.
- **Démonstration (2026-11-25)** : une ville bâtie de zéro sur une carte vierge de 768 m, uniquement par le MCP :
  colline et creux, avenue, 8 rues, route de colline, chemin, 6 quartiers + une zone libre, supérette et maison A,
  lotissement (86 bâtiments, 0 conflit), ligne électrique, 21 tronçons nommés, enregistrement, relance de l'éditeur
  et reprise à l'identique. Mesures : commandes 3 à 170 ms, captures 60 à 200 ms, recalcul de scène 622 ms.
- **Reste** : étape 4, test de CI qui refuse un binaire de release contenant la feature `remote`, outils
  `profil_route` / `relief_stats`.
