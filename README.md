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

Au premier lancement, **Configurer mon iPhone** démarre une étape de setup : SwagEx génère son certificat local une seule fois, affiche le lien d’installation, puis guide l’installation, l’approbation du certificat et le proxy Wi-Fi. Le bouton **J’ai terminé la configuration** enregistre ce setup.

Les ouvertures suivantes affichent directement **Exporter un nouvel JSON**. Cette action réutilise le même certificat et attend une nouvelle connexion au jeu ; le proxy s’arrête automatiquement dès que le profil est exporté dans Téléchargements.

Le lien **Refaire la configuration iPhone** est disponible si le téléphone ou le réseau doivent être reconfigurés. Cette action volontaire relance le setup avec le même certificat local ; le certificat n’est donc pas régénéré à chaque export, ni à chaque reprise de configuration.

Le produit est communautaire et non officiel. SwagEx ne modifie pas le jeu et ne doit pas être utilisé pour automatiser des actions de jeu.
