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

Au premier lancement, SwagEx démarre l’étape **Certificat de l’iPhone** : le certificat local est généré une seule fois, puis l’application guide l’installation et l’activation sur l’iPhone. L’écran **Proxy Wi-Fi de l’iPhone** affiche ensuite les valeurs à saisir.

Cet écran proxy est affiché à chaque ouverture et à chaque export, car l’iPhone ne conserve pas la configuration manuelle du proxy. Le bouton **J’ai configuré le proxy** démarre immédiatement l’écoute ; l’utilisateur peut alors lancer Summoners War. Le certificat reste mémorisé et n’est pas régénéré à chaque export.

Le lien **Nouveau certificat ?** est disponible depuis l’écran proxy. Il relance volontairement l’installation du certificat et doit être utilisé si le certificat a été supprimé ou si l’iPhone n’accorde plus sa confiance à l’ancien certificat.

Après capture, le nom du fichier JSON et son icône sont cliquables pour afficher le dossier correspondant dans le Finder, puis **Quitter SwagEx** ferme l’application.

Le produit est communautaire et non officiel. SwagEx ne modifie pas le jeu et ne doit pas être utilisé pour automatiser des actions de jeu.

## Mises à jour

SwagEx intègre le plugin Tauri Updater. L’application vérifie discrètement les nouvelles versions au lancement et expose aussi **SwagEx → Rechercher les mises à jour…** dans le menu macOS. Les mises à jour sont téléchargées uniquement depuis le manifeste GitHub Releases configuré dans `src-tauri/tauri.conf.json`, puis vérifiées par signature avant installation.

La clé privée de signature ne doit jamais être ajoutée au dépôt. Pour les publications, le workflow GitHub attend les secrets `TAURI_SIGNING_PRIVATE_KEY` et `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Le dépôt de publication est `mx-design-ux/SwagEx`. Les artefacts macOS doivent également être signés et notarisés avant une distribution large.

Une première installation manuelle reste nécessaire pour passer à cette version équipée de l’updater. Ensuite, une release publiée avec un tag tel que `v0.2.0` sera proposée directement aux utilisateurs déjà installés.
