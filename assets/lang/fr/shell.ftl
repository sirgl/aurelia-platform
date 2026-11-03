# Interface du jeu : menu, réglages, cadre de l'écran de jeu, mode observateur.

## shell
shell.turn = Tour
shell.turn.your-move = À vous de jouer
shell.turn.not-your-move = Ce n'est pas votre tour
shell.end-turn = Fin du tour
shell.end-turn-tip = Terminez votre tour ; les autres nations jouent
shell.game-over = Partie terminée
shell.others-moving = Les autres jouent…
shell.empire = Empire
shell.navigate = Navigation
shell.back = Retour
shell.back-to-map = Retour à la carte
shell.world-map = Carte du monde
shell.menu = Menu
shell.settings = Paramètres
shell.grows = En croissance
shell.stagnant = Stagnante
shell.choose-production = Choisir la production

## shell.main-menu
shell.main-menu.resume = Continuer
shell.main-menu.new-game = Nouvelle partie
shell.main-menu.save-game = Sauvegarder
shell.main-menu.load-game = Charger
shell.main-menu.quit = Quitter
shell.main-menu.report-bug = Signaler un bogue
shell.main-menu.your-world = VOTRE MONDE
shell.main-menu.explore = Explorer Rivermere
shell.main-menu.continue-journey = Poursuivre le voyage
shell.main-menu.gallery = Illustrations des nations
shell.main-menu.showcase-title = RIVERMERE
shell.main-menu.showcase-line = Un empire prend forme

## shell.report
shell.report.title = Signaler un bogue
shell.report.intro = Le bouton ci-dessous ouvre un ticket GitHub dont le texte que vous voyez ici est déjà rempli. Le jeu n'envoie rien : vous lisez la page et appuyez vous-même sur « Submit ». Votre nom, vos dossiers et votre adresse réseau n'y figurent pas.
shell.report.preview = Ce qui ira dans le ticket
shell.report.hint = Enregistrez d'abord le fichier de rapport, puis glissez-le dans le ticket : il contient ces informations et votre partie, pour que les développeurs puissent la rejouer.
shell.report.open-issue = Ouvrir la page du ticket
shell.report.save-file = Enregistrer le fichier de rapport
shell.report.copy-path = Copier le chemin
shell.report.copied = Chemin copié
shell.report.close = Fermer
shell.report.saved-path = Enregistré : { $place }
shell.report.saved-download = Téléchargé : { $place }. Glissez-le dans le ticket.
shell.report.cannot-write = Impossible d'enregistrer le fichier de rapport : { $error }
shell.report.crashed-title = Aurelia s'est arrêtée
shell.report.crashed-intro = Le jeu a rencontré une erreur et ne peut pas continuer. Signalez-la : le bouton ouvre un ticket GitHub avec le texte ci-dessous, et rien n'est envoyé tant que vous n'appuyez pas sur « Submit ». Rechargez la page pour rejouer.
shell.report.crashed-saved = Fichier de rapport téléchargé. Glissez-le dans le ticket.
shell.report.reload = Recharger la page

## shell.saves
shell.saves.wait-for-turns = Attendez que les autres nations aient terminé leur tour
shell.saves.saved = Sauvegardé au tour { $turn }
shell.saves.loaded = Tour { $turn } chargé
shell.saves.nothing-saved = Aucune partie sauvegardée pour l'instant
shell.saves.loaded-world = Monde chargé
shell.saves.other-module = Cette sauvegarde a été créée avec les règles { $saved } ; cette partie utilise les règles { $running }
shell.saves.unknown-module = Ce fichier n'indique pas avec quelles règles il a été sauvegardé
shell.saves.not-persistent = Les parties ne seront pas conservées : ce navigateur n'autorise pas le stockage (fenêtre privée ?)
shell.saves.load-title = Charger une partie
shell.saves.save-title = Sauvegarder la partie
shell.saves.close = Fermer
shell.saves.empty = Aucune partie sauvegardée pour le moment
shell.saves.load = Charger
shell.saves.delete = Supprimer
shell.saves.delete-ask = Supprimer « { $name } » ?
shell.saves.delete-yes = Supprimer
shell.saves.cancel = Annuler
shell.saves.deleted = Sauvegarde supprimée
shell.saves.not-in-list = cette sauvegarde n'est pas dans la liste
shell.saves.mark-auto = Auto
shell.saves.mark-quick = Rapide
shell.saves.name-quick = Sauvegarde rapide
shell.saves.name-auto = Sauvegarde auto
shell.saves.row-turn = Tour { $turn }
shell.saves.unreadable = Illisible
shell.saves.reading = Lecture…
shell.saves.problem-newer = Sauvegardée avec une version plus récente du jeu (format { $format })
shell.saves.problem-older = Sauvegardée avec une version plus ancienne que ce jeu ne lit plus (format { $format })
shell.saves.problem-damaged = Le fichier est endommagé : { $detail }
shell.saves.age-now = À l'instant
shell.saves.age-minutes = { $n ->
    [one] il y a { $n } minute
   *[other] il y a { $n } minutes
}
shell.saves.age-hours = { $n ->
    [one] il y a { $n } heure
   *[other] il y a { $n } heures
}
shell.saves.age-days = { $n ->
    [one] il y a { $n } jour
   *[other] il y a { $n } jours
}
shell.saves.age-date = { DATE($date) }
shell.saves.name-label = Nom de la sauvegarde
shell.saves.name-replaces = Une sauvegarde porte déjà ce nom ; elle sera remplacée
shell.saves.name-save = Sauvegarder
shell.saves.name-replace = Remplacer
shell.saves.default-name = { $civ }, tour { $turn }
shell.saves.default-name-plain = Tour { $turn }

## shell.options
shell.options.audio = Audio
shell.options.graphics = Graphismes
shell.options.game = Jeu
shell.options.controls = Commandes
shell.options.applied = Les changements s'appliquent immédiatement et sont enregistrés automatiquement.
shell.options.off = Désactivé
shell.options.on = Activé
shell.options.master-volume.label = Volume général
shell.options.master-volume.help = Volume global de la musique et des effets sonores.
shell.options.music-volume.label = Volume de la musique
shell.options.music-volume.help = La musique qui accompagne votre nation.
shell.options.mute-music.label = Couper la musique
shell.options.mute-music.help = Coupe la musique sans modifier son volume.
shell.options.effects-volume.label = Volume des effets
shell.options.effects-volume.help = Sons de l'interface et événements sur la carte.
shell.options.mute-effects.label = Couper les effets sonores
shell.options.mute-effects.help = Coupe les effets sonores sans toucher à la musique.
shell.options.audio-failed = Certains sons n'ont pas pu être lus.
shell.options.window-mode.label = Mode d'affichage
shell.options.window-mode.help = Choisissez une fenêtre ou le plein écran.
shell.options.windowed = Fenêtré
shell.options.fullscreen = Plein écran
shell.options.window-size.label = Taille de la fenêtre
shell.options.window-size.help = Taille de la fenêtre de jeu.
shell.options.window-size-fullscreen = Le plein écran utilise la résolution de votre écran.
shell.options.quality = Qualité graphique
shell.options.quality-low.label = Basse
shell.options.quality-low.help = Moins de détails au loin ; animation de l'eau en pause.
shell.options.quality-medium.label = Moyenne
shell.options.quality-medium.help = Détails équilibrés, eau et rivières animées.
shell.options.quality-high.label = Haute
shell.options.quality-high.help = Tous les détails à toute distance, eau animée.
shell.options.render-scale.label = Échelle de rendu
shell.options.render-scale.help = Netteté du dessin de la carte avant son ajustement à la fenêtre ; une valeur plus basse est plus rapide sur les cartes graphiques modestes. L'interface reste nette.
shell.options.auto = Auto
shell.options.anti-aliasing.label = Anticrénelage
shell.options.anti-aliasing.help = Adoucit les bords crénelés de la carte ; désactivé, c'est plus rapide sur les cartes graphiques modestes.
shell.options.interface-scale.label = Échelle de l'interface
shell.options.interface-scale.help = Ajuste la taille du texte et des commandes.
shell.options.brightness.label = Luminosité des illustrations
shell.options.brightness.help = Luminosité des illustrations dans les menus et les écrans.
shell.options.autosave.label = Sauvegarde auto
shell.options.autosave.help = Sauvegarde automatiquement votre progression à la fin d'un tour.
shell.options.never = Jamais
shell.options.every-turn = Chaque tour
shell.options.every-turns = { $n ->
    [one] Chaque tour
   *[other] Tous les { $n } tours
}
shell.options.saves-kept.label = Sauvegardes à conserver
shell.options.saves-kept.help = Les plus anciennes sauvegardes automatiques sont remplacées quand cette limite est atteinte.
shell.options.other-turns.label = Tours des autres nations
shell.options.other-turns.help = Vitesse d'affichage des actions sur la carte.
shell.options.speed-normal = Normale
shell.options.speed-fast = Rapide
shell.options.speed-skip = Passer
shell.options.reduced-motion.label = Animations réduites
shell.options.reduced-motion.help = Met en pause les animations de marche et de combat ainsi que l'eau en mouvement ; désactive les glissements de caméra.
shell.options.wonder-camera.label = Caméra des merveilles
shell.options.wonder-camera.help = Déplace la caméra vers une merveille achevée.
shell.options.wonder-camera-off = Désactivée tant que les animations réduites sont activées.
shell.options.language.label = Langue
shell.options.language.help = Langue des menus, des écrans et des textes du jeu. Le changement est immédiat ; « Automatique » suit votre système.
shell.options.language-auto = Automatique
shell.options.language-custom = paquet personnalisé
shell.options.web-mode.label = Mode d'interface
shell.options.web-mode.help = Le mode téléphone et tablette a de plus grands boutons et des gestes tactiles ; le mode ordinateur est pour la souris et le clavier. La page se recharge et la partie en cours se termine.
shell.options.web-mode.mobile = Téléphone et tablette
shell.options.web-mode.desktop = Ordinateur
shell.options.language-status.machine-drafted = Traduction automatique : aucun locuteur natif ne l'a encore relue.
shell.options.language-status.reviewed = Traduction relue.
shell.options.language-status.community = Traduction de la communauté.
shell.options.language-status.test = Langue de test pour vérifier la mise en page.
shell.options.controls-help = Glisser pour déplacer la carte · Molette pour zoomer · Cliquer sur une unité ou une ville pour la sélectionner

## shell.hud
shell.hud.settings-tip = Paramètres
shell.hud.center-capital = Centrer sur la capitale
shell.hud.toggle-grid = Afficher ou masquer la grille hexagonale
shell.hud.toggle-labels = Afficher ou masquer les noms des villes
shell.hud.toggle-yields = Afficher ou masquer les rendements des cases
shell.hud.minimap-tip = Cliquez pour déplacer la caméra
shell.hud.orders-unavailable = Ordres indisponibles
shell.hud.turn-unavailable = La progression des tours n'est pas disponible dans cette version.
shell.hud.units = Unités
shell.hud.garrison = Garnison
shell.hud.this-turn = Ce tour
shell.hud.turns = { $n ->
    [one] { $n } tour
   *[other] { $n } tours
}
shell.hud.captured = Capturé
shell.hud.destroyed = Détruit
shell.hud.health = Santé { $current }/{ $max }
shell.hud.strength = Force { $attack } en attaque · { $defense } en défense
shell.hud.ranged = À distance { $strength } · portée { $range }
shell.hud.moves = Mouvement { $moves }
shell.hud.sight = Vision { $sight }
shell.hud.population = { $name } · population { $population }
shell.hud.combat-strength = Force de combat { $strength }
shell.hud.food = Nourriture { $stored }/{ $needed }
shell.hud.grows-in = { $n ->
    [one] croissance dans { $n } tour
   *[other] croissance dans { $n } tours
}
shell.hud.not-growing = pas de croissance
shell.hud.producing = Production : { $name } { $progress }/{ $cost }
shell.hud.manage-city = Cliquez pour gérer la ville
shell.hud.hit-points = Points de vie { $current }/{ $max }
shell.hud.strike-ready = Arc : frappe une unité à 2 cases ou moins

## shell.observer
shell.observer.entry = Observateur
shell.observer.entry-tip = Regardez les nations de l'IA jouer toute la partie : la vue propre à chaque nation, sans ordres
shell.observer.turn-of = Tour { $round } sur { $limit }
shell.observer.turn = Tour { $round }
shell.observer.pause = Pause
shell.observer.resume = Reprendre
shell.observer.step = Avancer
shell.observer.paused = en pause
shell.observer.speed-1 = ×0.25
shell.observer.speed-2 = ×0.5
shell.observer.speed-3 = ×1
shell.observer.speed-4 = ×2
shell.observer.speed-5 = ×4
shell.observer.speed-6 = ×8
shell.observer.speed-7 = ×16
shell.observer.speed-8 = Max
shell.observer.slower-tip = Plus lent ([)
shell.observer.faster-tip = Plus rapide (])
shell.observer.step-tip = Jouer un tour en pause (N)
shell.observer.pause-tip = Mettre en pause ou reprendre (Espace)
shell.observer.fallen = éliminée
shell.observer.grid-hint = Survolez une case pour voir ce qu'elle contient · cliquez sur un panneau ou appuyez sur 1-8 pour suivre une nation · R : toute la carte
shell.observer.zoom-hint = Survol : la case · cliquez sur une ville ou une unité · C : villes · R : toute la carte · Tab ou flèches : changer de nation · 1-8 : nation · G : grille · Esc : retour
shell.observer.view-single = Une nation
shell.observer.view-grid = Grille
shell.observer.view-single-tip = Suivre une seule nation en grand (G)
shell.observer.view-grid-tip = Retour à la grille de toutes les nations (G ou Esc)
shell.observer.camera-follow = Suivre
shell.observer.camera-free = Caméra libre
shell.observer.camera-follow-tip = La caméra cadre les villes et la vision de la nation (F)
shell.observer.camera-free-tip = Faites glisser la carte, tournez la molette : la caméra reste où vous la laissez
shell.observer.watch-all = Tout observer
shell.observer.watch-one = En observer une
shell.observer.watch-tip = Regardez l'IA jouer : la grille de toutes les nations, ou une nation seule
shell.observer.events-title = Événements récents
shell.observer.no-events = Rien pour l'instant
shell.observer.event-line = T{ $round } : { $text }
shell.observer.ev-founded = Fondation de { $city }
shell.observer.ev-captured = { $city } prise à { $nation }
shell.observer.ev-captured-plain = { $city } prise
shell.observer.ev-lost = { $city } perdue au profit de { $nation }
shell.observer.ev-lost-plain = { $city } perdue
shell.observer.ev-war = Guerre contre { $nation }
shell.observer.ev-peace = Paix avec { $nation }
shell.observer.ev-learned = Découverte : { $tech }
shell.observer.ev-fell = Éliminée
shell.observer.finale-title = Fin de la partie
shell.observer.winner = { $name } a gagné
shell.observer.winners = { $names } ont gagné ensemble
shell.observer.nobody = Personne n'a gagné
shell.observer.finale-line = Tour { $round } · { $reason }
shell.observer.cannot-play = Une partie observée ne peut pas être poursuivie
shell.observer.keep-playing = Un tour de plus
shell.observer.to-menu = Menu principal
shell.observer.final-table = CLASSEMENT FINAL
shell.observer.no-save = Une partie observée ne peut pas être sauvegardée
shell.observer.reveal = Toute la carte
shell.observer.reveal-tip = Ouvre toute la carte dans chaque vue : sans brouillard, villes et unités dans leur état actuel (R)
shell.observer.reveal-tag = Carte entière ouverte · R : rétablir le brouillard
shell.observer.cities = Villes
shell.observer.cities-tip = Fiches des villes de la nation : ce qu'elles contiennent, sans ordres (C)
shell.observer.city-place = Ville { $place } sur { $count }
shell.observer.card-hint = Flèches gauche et droite ou 1-9 : autre ville · Esc : fermer
shell.observer.card-close = Fermer
shell.observer.card-close-tip = Fermer la fiche (Esc)
shell.observer.card-prev-tip = La ville précédente (flèche gauche)
shell.observer.card-next-tip = La ville suivante (flèche droite)
shell.observer.city-click = Cliquez pour ouvrir la fiche de la ville
shell.observer.units-title = Unités
shell.observer.unit-name = { $kind } ({ $nation })
shell.observer.unit-health = santé { $current } sur { $max }
shell.observer.hp-value = { $current } sur { $max }
shell.observer.unit-sight = vision { $sight }
shell.observer.sight-unknown = vision inconnue
shell.observer.unit-fortified = fortifiée
shell.observer.sight-label = Vision
shell.observer.health-label = Santé
shell.observer.nation-label = Nation
shell.observer.unit-hint = Esc : annuler la sélection
shell.observer.sight-note = Le contour montre ce qu'elle voit
shell.observer.key-pause.keys = Espace
shell.observer.key-pause.what = Mettre en pause et reprendre
shell.observer.key-speed.keys = [ et ]
shell.observer.key-speed.what = Plus lent et plus rapide
shell.observer.key-step.keys = N
shell.observer.key-step.what = Jouer un tour en pause
shell.observer.key-grid.keys = G
shell.observer.key-grid.what = La grille de toutes les nations, ou une seule nation
shell.observer.key-tab.keys = Tab, Shift+Tab
shell.observer.key-tab.what = Une autre nation (une nation en grand)
shell.observer.key-arrows.keys = Flèches gauche et droite
shell.observer.key-arrows.what = Une autre nation ; avec une fiche de ville ouverte, une autre ville
shell.observer.key-numbers.keys = 1-8
shell.observer.key-numbers.what = Une nation selon son numéro ; avec une fiche de ville ouverte, une ville selon son numéro
shell.observer.key-cities.keys = C
shell.observer.key-cities.what = Fiches des villes de la nation
shell.observer.key-reveal.keys = R
shell.observer.key-reveal.what = Toute la carte ouverte, ou le retour du brouillard
shell.observer.key-follow.keys = F
shell.observer.key-follow.what = La caméra suit le territoire de la nation
shell.observer.key-hover.keys = Survoler une case
shell.observer.key-hover.what = Ce qu'elle contient : terrain, rendements, aménagement, ressource, propriétaire, unités
shell.observer.key-click-city.keys = Cliquer sur une ville
shell.observer.key-click-city.what = Sa fiche : ce qu'elle contient, sans ordres
shell.observer.key-click-unit.keys = Cliquer sur une unité
shell.observer.key-click-unit.what = La sélectionner : contour de sa vision et ses détails
shell.observer.key-escape.keys = Esc
shell.observer.key-escape.what = Fermer la fiche, annuler la sélection, retour à la grille

## Lands & Laws dit faction là où le jeu de base dit nation
shell.end-turn-tip@lal = Terminez votre tour ; les autres factions jouent, puis l'économie du tour est calculée
shell.main-menu.gallery@lal = Illustrations des factions
shell.saves.wait-for-turns@lal = Attendez que les autres factions aient terminé leur tour
shell.options.music-volume.help@lal = La musique qui accompagne votre faction.
shell.options.other-turns.label@lal = Tours des autres factions
shell.observer.entry-tip@lal = Regardez les factions de l'IA jouer toute la partie : la vue propre à chaque faction, sans ordres
shell.observer.grid-hint@lal = Survolez une case pour voir ce qu'elle contient · cliquez sur un panneau ou appuyez sur 1-8 pour suivre une faction · R : toute la carte
shell.observer.zoom-hint@lal = Survol : la case · cliquez sur une ville ou une unité · C : villes · R : toute la carte · Tab ou flèches : changer de faction · 1-8 : faction · G : grille · Esc : retour
shell.observer.view-single@lal = Une faction
shell.observer.view-single-tip@lal = Suivre une seule faction en grand (G)
shell.observer.view-grid-tip@lal = Retour à la grille de toutes les factions (G ou Esc)
shell.observer.camera-follow-tip@lal = La caméra cadre les villes et la vision de la faction (F)
shell.observer.watch-tip@lal = Regardez l'IA jouer : la grille de toutes les factions, ou une faction seule
shell.observer.cities-tip@lal = Fiches des villes de la faction : ce qu'elles contiennent, sans ordres (C)
shell.observer.nation-label@lal = Faction
shell.observer.key-grid.what@lal = La grille de toutes les factions, ou une seule faction
shell.observer.key-tab.what@lal = Une autre faction (une faction en grand)
shell.observer.key-arrows.what@lal = Une autre faction ; avec une fiche de ville ouverte, une autre ville
shell.observer.key-numbers.what@lal = Une faction selon son numéro ; avec une fiche de ville ouverte, une ville selon son numéro
shell.observer.key-cities.what@lal = Fiches des villes de la faction
shell.observer.key-follow.what@lal = La caméra suit le territoire de la faction

## Added after the first draft: cover, save drift notice, file errors, showcase sample
shell.cover.kicker = UN JEU DE STRATÉGIE
shell.cover.tagline = Un monde qui n'attend qu'à être écrit
shell.saves.drift = Sauvegardé avec d'autres règles ({ $saved }) ; chargé avec les règles actuelles ({ $current }) ; { $changed }
shell.saves.drift-changed = modifié : { $parts }
shell.saves.drift-none = aucune partie ne diffère
shell.saves.drift-unknown = la sauvegarde n'indique pas quelles parties ont changé
shell.errors.cannot-create = Impossible de créer { $path } : { $error }
shell.errors.cannot-read = Impossible de lire { $path } : { $error }
shell.errors.cannot-remove = Impossible de supprimer { $path } : { $error }
shell.errors.cannot-read-save = Impossible de lire la sauvegarde : { $error }
shell.errors.cannot-save = Impossible de sauvegarder : { $error }
shell.errors.storage-full = le stockage du navigateur est plein ; supprimez d'anciennes sauvegardes ou libérez de l'espace pour ce site
shell.errors.cannot-rotate = Impossible de renouveler les sauvegardes automatiques : { $error }
shell.errors.cannot-write-journal = Impossible d'écrire le journal des ordres : { $error }
shell.errors.cannot-save-settings = Impossible d'enregistrer les réglages : { $error }
shell.errors.hall-open = Impossible d'ouvrir le Temple de la renommée : { $error }
shell.errors.hall-write = Impossible d'écrire dans le Temple de la renommée : { $error }
shell.showcase.research = RECHERCHE
shell.showcase.research-name = Extraction minière
shell.showcase.research-turns = 3 tours
