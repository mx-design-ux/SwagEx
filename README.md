# SwagEx

SwagEx est un exporteur local minimal pour macOS et Windows. Sa seule fonction est de produire un fichier JSON du compte compatible avec les outils qui acceptent les exports SWEX.

SwagEx ne contient aucun optimiseur, aucune logique SWAG, aucun plugin et aucun service distant. Le fichier JSON appartient à l’utilisateur et reste sur son ordinateur.

## Développement

Pré-requis : Rust et Node.js. Sur macOS, installez aussi les outils de ligne de commande Xcode ; sur Windows, les Microsoft C++ Build Tools si Rust ne les a pas déjà installés.

```bash
npm install
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri dev
```

Pour produire l’application du système courant :

```bash
npm run tauri build
```

Le JSON de référence utilisé pour la compatibilité doit rester hors du dépôt. Il sert uniquement à comparer la structure et les clés de l’export, jamais à alimenter l’application distribuée.

L’export applique le contrat d’ordre de SWEX sur une copie du profil, juste avant l’écriture du fichier : le stockage est placé en dernier dans `unit_list`, puis les unités sont ordonnées par classe décroissante, niveau décroissant, attribut croissant et identifiant croissant. Les runes d’un monstre suivent `slot_no` puis `rune_id`, les runes d’inventaire suivent `set_id`, `slot_no` puis `rune_id`, et les crafts, artefacts et reliques suivent leurs identifiants déterministes. Les formes objet de `runes` sont converties en tableaux dans le fichier exporté. Le profil capturé et la réponse relayée à l’appareil Apple ne sont jamais modifiés ; les listes à ordre métier (decks, compétences et sous-statistiques) conservent leur ordre reçu.

## Parcours iOS / Mac ou PC

Au premier lancement, SwagEx demande l’appareil de jeu. Le parcours **iOS** couvre iPhone et iPad. Il génère une seule fois le certificat local puis guide son installation. Le QR code télécharge un profil de configuration `.mobileconfig` contenant uniquement le certificat public. Sur l’appareil Apple, enregistrez-le dans **Fichiers**, ouvrez **SwagEx.mobileconfig**, puis choisissez votre appareil. L’écran proxy affiche ensuite les valeurs à saisir.

Après le téléchargement du profil, iOS impose encore deux actions manuelles : toucher **Installer** dans **Réglages → Profil téléchargé**, puis activer **SwagEx** dans **Réglages → Général → Informations → Réglages des certificats**. Cette confiance ne peut pas être activée automatiquement pour un certificat installé manuellement sur un appareil Apple non supervisé.

Cet écran proxy est affiché à chaque ouverture et à chaque export, car iOS ne conserve pas la configuration manuelle du proxy. Le bouton **C’est configuré !** démarre immédiatement l’écoute ; l’utilisateur peut alors lancer Summoners War. Le certificat reste mémorisé et n’est pas régénéré à chaque export.

Le lien **Nouveau certificat ?** est disponible depuis l’écran proxy. Il relance volontairement l’installation du certificat et doit être utilisé si le certificat a été supprimé ou si l’appareil Apple n’accorde plus sa confiance à l’ancien certificat. En dehors de cette action explicite, SwagEx ne remplace jamais silencieusement la CA : si ses fichiers persistants sont incomplets, l’application s’arrête avec une erreur au lieu de créer un nouveau certificat qui invaliderait celui déjà installé. Un certificat déjà installé conserve son nom existant ; cliquer sur ce lien génère le nouveau certificat nommé **SwagEx** et impose donc une réinstallation et une nouvelle activation de confiance.

## Parcours Steam / Windows

Le parcours **Steam** est disponible dans l’application Windows. Lors de la première utilisation, **Installer le certificat** ouvre le certificat DER local dans le visualiseur natif de Windows. Le certificat doit être placé dans **Autorités de certification racines de confiance**. SwagEx compare ensuite le certificat exact généré par l’application aux magasins racines de l’utilisateur et de l’ordinateur. L’écoute Steam ne peut pas démarrer tant que Windows ne confirme pas cette confiance ; supprimer le certificat fait automatiquement réapparaître l’écran d’installation.

La version Windows demande les droits administrateur au lancement afin de pouvoir rediriger temporairement les domaines régionaux de Summoners War vers le proxy local. SwagEx délimite son propre bloc dans le fichier `hosts`, conserve toutes les autres lignes et retire sa redirection à l’arrêt de l’écoute, après la capture ou à la fermeture de l’application. Le certificat est mémorisé et l’écran d’installation est ignoré lors des utilisations suivantes.

Le parcours Android reste présent dans le modèle technique, mais sa carte est masquée dans le sélecteur tant que sa compatibilité sur les appareils récents n’est pas validée.

Le menu **SwagEx → Changer mon appareil de jeu…** ramène au choix de l’appareil sans régénérer ni supprimer le certificat existant.

Depuis la version 0.3.0, l’identifiant distribué est neutre (`app.swagex.desktop`) et les données fonctionnelles résident dans un dossier simplement nommé `SwagEx`. Au premier lancement, l’application déplace automatiquement les certificats, réglages et JSON créés par les versions antérieures. La migration refuse tout écrasement et conserve la même autorité de certification afin de ne pas invalider un certificat déjà approuvé.

Depuis la version 0.3.1, l’écran d’attente n’apparaît qu’après une courte fenêtre de stabilisation du proxy. Le texte d’attente et le fichier JSON final apparaissent avec une transition d’opacité de 200 ms.

Après capture, le nom du fichier JSON et son icône sont cliquables pour afficher le dossier correspondant dans Finder sur macOS ou l’Explorateur de fichiers sur Windows, puis **Quitter SwagEx** ferme l’application.

Le produit est communautaire et non officiel. SwagEx ne modifie pas le jeu et ne doit pas être utilisé pour automatiser des actions de jeu.

## Mises à jour

SwagEx intègre le plugin Tauri Updater. L’application vérifie discrètement les nouvelles versions au lancement et expose aussi **SwagEx → Rechercher les mises à jour…** dans son menu. Une même release contient les installateurs et mises à jour macOS Apple Silicon, macOS Intel et Windows 64 bits. Les mises à jour sont téléchargées uniquement depuis le manifeste GitHub Releases configuré dans `src-tauri/tauri.conf.json`, puis vérifiées par signature avant installation.

La clé privée de signature ne doit jamais être ajoutée au dépôt. Pour les publications, le workflow GitHub attend les secrets `TAURI_SIGNING_PRIVATE_KEY` et `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Le dépôt de publication est `mx-design-ux/SwagEx`. Les artefacts macOS sont signés ad hoc (`APPLE_SIGNING_IDENTITY=-`) : aucun compte Apple payant n'est requis, mais macOS demande à chaque utilisateur une autorisation initiale dans **Réglages Système → Confidentialité et sécurité → Ouvrir quand même**. Windows peut lui aussi afficher un avertissement SmartScreen lors de la première installation tant que l’application n’est pas signée avec un certificat de code public.

Une première installation manuelle reste nécessaire pour passer à cette version équipée de l’updater. Ensuite, une release publiée avec un tag tel que `v0.2.1` sera proposée directement aux utilisateurs déjà installés.

Pour que macOS propose SwagEx dans Spotlight, copiez `SwagEx.app` dans `/Applications` (et non depuis le volume du `.dmg`), puis ouvrez-la une première fois depuis ce dossier. Une application lancée directement depuis le `.dmg` peut ne pas rester disponible dans Spotlight après l’éjection du volume. Sur Windows, installez l’application avec l’installateur `.exe` publié par la release : elle sera alors disponible dans le menu Démarrer.
