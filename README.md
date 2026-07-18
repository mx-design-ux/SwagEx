# SwagEx

SwagEx est un exporteur local minimal pour macOS. Sa seule fonction est de produire un fichier JSON du compte compatible avec les outils qui acceptent les exports SWEX.

SwagEx ne contient aucun optimiseur, aucune logique SWAG, aucun plugin et aucun service distant. Le fichier JSON appartient à l’utilisateur et reste sur son Mac.

## Développement

Pré-requis : Rust, Node.js et les outils de ligne de commande Xcode.

```bash
npm install
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri dev
```

Pour produire une application Apple Silicon :

```bash
npm run tauri build
```

Le JSON de référence utilisé pour la compatibilité doit rester hors du dépôt. Il sert uniquement à comparer la structure et les clés de l’export, jamais à alimenter l’application distribuée.

## Flux iPhone / Mac

1. SwagEx démarre un proxy local uniquement sur l’adresse Wi-Fi du Mac.
2. L’utilisateur installe et approuve le certificat SwagEx sur son iPhone.
3. L’utilisateur configure le proxy Wi-Fi de l’iPhone avec l’adresse et le port affichés.
4. SwagEx capture le profil de connexion, vérifie qu’il est complet, puis écrit le JSON dans Téléchargements.
5. Le proxy s’arrête automatiquement dès que l’export est terminé.

Le produit est communautaire et non officiel. SwagEx ne modifie pas le jeu et ne doit pas être utilisé pour automatiser des actions de jeu.
