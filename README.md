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

L’export applique le contrat d’ordre de SWEX sur une copie du profil, juste avant l’écriture du fichier : le stockage est placé en dernier dans `unit_list`, puis les unités sont ordonnées par classe décroissante, niveau décroissant, attribut croissant et identifiant croissant. Les runes d’un monstre suivent `slot_no` puis `rune_id`, les runes d’inventaire suivent `set_id`, `slot_no` puis `rune_id`, et les crafts, artefacts et reliques suivent leurs identifiants déterministes. Les formes objet de `runes` sont converties en tableaux dans le fichier exporté. Le profil capturé et la réponse relayée à l’iPhone ne sont jamais modifiés ; les listes à ordre métier (decks, compétences et sous-statistiques) conservent leur ordre reçu.

## Flux iPhone / Mac

Au premier lancement, SwagEx démarre l’étape **Certificat de l’iPhone** : le certificat local est généré une seule fois, puis l’application guide l’installation et l’activation sur l’iPhone. Le QR code ouvre désormais un profil de configuration `.mobileconfig` contenant uniquement le certificat public ; Safari le remet directement à Réglages, sans passage par Fichiers ni partage manuel. L’écran **Proxy Wi-Fi de l’iPhone** affiche ensuite les valeurs à saisir.

Après le téléchargement du profil, iOS impose encore deux actions manuelles : toucher **Installer** dans **Réglages → Profil téléchargé**, puis activer **SwagEx Local CA** dans **Réglages → Général → Informations → Réglages des certificats**. Cette confiance ne peut pas être activée automatiquement pour un certificat installé manuellement sur un iPhone non supervisé.

Cet écran proxy est affiché à chaque ouverture et à chaque export, car l’iPhone ne conserve pas la configuration manuelle du proxy. Le bouton **J’ai configuré le proxy** démarre immédiatement l’écoute ; l’utilisateur peut alors lancer Summoners War. Le certificat reste mémorisé et n’est pas régénéré à chaque export.

Le lien **Nouveau certificat ?** est disponible depuis l’écran proxy. Il relance volontairement l’installation du certificat et doit être utilisé si le certificat a été supprimé ou si l’iPhone n’accorde plus sa confiance à l’ancien certificat. En dehors de cette action explicite, SwagEx ne remplace jamais silencieusement la CA : si ses fichiers persistants sont incomplets, l’application s’arrête avec une erreur au lieu de créer un nouveau certificat qui invaliderait celui déjà installé sur l’iPhone.

Après capture, le nom du fichier JSON et son icône sont cliquables pour afficher le dossier correspondant dans le Finder, puis **Quitter SwagEx** ferme l’application.

Le produit est communautaire et non officiel. SwagEx ne modifie pas le jeu et ne doit pas être utilisé pour automatiser des actions de jeu.

## Mises à jour

SwagEx intègre le plugin Tauri Updater. L’application vérifie discrètement les nouvelles versions au lancement et expose aussi **SwagEx → Rechercher les mises à jour…** dans le menu macOS. Les mises à jour sont téléchargées uniquement depuis le manifeste GitHub Releases configuré dans `src-tauri/tauri.conf.json`, puis vérifiées par signature avant installation.

La clé privée de signature ne doit jamais être ajoutée au dépôt. Pour les publications, le workflow GitHub attend les secrets `TAURI_SIGNING_PRIVATE_KEY` et `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Le dépôt de publication est `mx-design-ux/SwagEx`. Les artefacts macOS sont signés ad hoc (`APPLE_SIGNING_IDENTITY=-`) : aucun compte Apple payant n'est requis, mais macOS demande à chaque utilisateur une autorisation initiale dans **Réglages Système → Confidentialité et sécurité → Ouvrir quand même**.

Une première installation manuelle reste nécessaire pour passer à cette version équipée de l’updater. Ensuite, une release publiée avec un tag tel que `v0.2.0` sera proposée directement aux utilisateurs déjà installés.

Pour que macOS propose SwagEx dans Spotlight, copiez `SwagEx.app` dans `/Applications` (et non depuis le volume du `.dmg`), puis ouvrez-la une première fois depuis ce dossier. Une application lancée directement depuis le `.dmg` peut ne pas rester disponible dans Spotlight après l’éjection du volume.
