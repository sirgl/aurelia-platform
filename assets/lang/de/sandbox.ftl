# Translation of sandbox.ftl into de. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

sandbox.lib.title = Sandbox
sandbox.lib.found-city = Stadt gründen
sandbox.lib.match-summary = { $faction } · Welt von { $width } × { $height }
sandbox.lib.unit-line = { $hp } TP · { $moves ->
    [one] { $moves } Bewegungspunkt
   *[other] { $moves } Bewegungspunkte
}
sandbox.lib.unit-line-civilian = { $hp } TP · { $moves ->
    [one] { $moves } Bewegungspunkt
   *[other] { $moves } Bewegungspunkte
} · Zivileinheit
sandbox.lib.hall-of-fame = Sandbox · { $faction } · { $outcome } in Zug { $round }
sandbox.lib.outcome-playing = Im Spiel
sandbox.lib.outcome-victory = Sieg
sandbox.lib.outcome-defeat = Niederlage
sandbox.lib.key-left-click = Linksklick
sandbox.lib.key-left-click-help = Einheit auswählen oder Stadt öffnen
sandbox.lib.key-right-click = Rechtsklick
sandbox.lib.key-right-click-help = Einheit dorthin bewegen oder angreifen
sandbox.screens.city-title = Stadt
sandbox.screens.new-game-title = Neues Sandbox-Spiel
sandbox.screens.sandbox-heading = Sandbox
sandbox.screens.about = Ein kleines 4X-Spiel auf dem Kern der Plattform: Vier Fraktionen gründen Städte, lassen sie mit Nahrung und Produktion wachsen und kämpfen um jede Hauptstadt. Wer in Zug 100 das größte Reich hat, gewinnt.
sandbox.screens.start-game = Spiel starten
sandbox.screens.choose-city = Wählen Sie eine Ihrer Städte auf der Karte.
sandbox.screens.city-line = Bevölkerung { $population } · { $food } Nahrung · { $production } Produktion
sandbox.screens.producing = IN PRODUKTION
sandbox.screens.producing-item = { $item } — { $stored } angesammelt
sandbox.screens.producing-nothing = Nichts: Wählen Sie, was gebaut werden soll
sandbox.screens.build-next = ALS NÄCHSTES BAUEN
sandbox.screens.turns = { $n ->
    [one] { $n } Zug
   *[other] { $n } Züge
}
sandbox.screens.never = nie
sandbox.screens.offer = { $name }   { $cost } · { $turns }
sandbox.screens.esc-hint = Esc führt zurück zur Karte.
sandbox.session.not-a-save = Kein Sandbox-Spielstand ({ $detail })
sandbox.session.save-failed = Der Spielstand lässt sich nicht laden ({ $detail })
sandbox.session.top-food = { $amount } Nahrung
sandbox.session.top-production = { $amount } Produktion
sandbox.session.top-cities = { $n ->
    [one] { $n } Stadt
   *[other] { $n } Städte
}
