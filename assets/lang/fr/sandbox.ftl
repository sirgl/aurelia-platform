# Translation of sandbox.ftl into fr. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

sandbox.lib.title = Bac à sable
sandbox.lib.found-city = Fonder une ville
sandbox.lib.match-summary = { $faction } · monde de { $width } × { $height }
sandbox.lib.unit-line = { $hp } PV · { $moves ->
    [one] { $moves } mouvement
   *[other] { $moves } mouvements
}
sandbox.lib.unit-line-civilian = { $hp } PV · { $moves ->
    [one] { $moves } mouvement
   *[other] { $moves } mouvements
} · civile
sandbox.lib.hall-of-fame = Bac à sable · { $faction } · { $outcome } au tour { $round }
sandbox.lib.outcome-playing = En cours
sandbox.lib.outcome-victory = Victoire
sandbox.lib.outcome-defeat = Défaite
sandbox.lib.key-left-click = Clic gauche
sandbox.lib.key-left-click-help = Sélectionner une unité ou ouvrir une ville
sandbox.lib.key-right-click = Clic droit
sandbox.lib.key-right-click-help = Y déplacer l'unité, ou attaquer
sandbox.screens.city-title = Ville
sandbox.screens.new-game-title = Nouvelle partie en bac à sable
sandbox.screens.sandbox-heading = Bac à sable
sandbox.screens.about = Un petit 4X bâti sur le noyau de la plateforme : quatre factions fondent des villes, les font croître grâce à la nourriture et à la production, et se disputent chaque capitale. Le plus grand empire au tour 100 l'emporte.
sandbox.screens.start-game = Lancer la partie
sandbox.screens.choose-city = Choisissez l'une de vos villes sur la carte.
sandbox.screens.city-line = Population { $population } · { $food } nourriture · { $production } production
sandbox.screens.producing = EN PRODUCTION
sandbox.screens.producing-item = { $item } — { $stored } en réserve
sandbox.screens.producing-nothing = Rien : choisissez quoi construire
sandbox.screens.build-next = CONSTRUIRE ENSUITE
sandbox.screens.turns = { $n ->
    [one] { $n } tour
   *[other] { $n } tours
}
sandbox.screens.never = jamais
sandbox.screens.offer = { $name }   { $cost } · { $turns }
sandbox.screens.esc-hint = Esc revient à la carte.
sandbox.session.not-a-save = Ce n'est pas une sauvegarde du bac à sable ({ $detail })
sandbox.session.save-failed = La sauvegarde ne se charge pas ({ $detail })
sandbox.session.top-food = { $amount } nourriture
sandbox.session.top-production = { $amount } production
sandbox.session.top-cities = { $n ->
    [one] { $n } ville
   *[other] { $n } villes
}
