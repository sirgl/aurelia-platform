# The sandbox module: its menu entry, screens, unit panel and HUD values. Source of truth: en.

## sandbox.lib
sandbox.lib.title = Sandbox
sandbox.lib.found-city = Found city
sandbox.lib.match-summary = { $faction } · { $width } × { $height } world
sandbox.lib.unit-line = { $hp } HP · { $moves ->
    [one] { $moves } move
   *[other] { $moves } moves
}
sandbox.lib.unit-line-civilian = { $hp } HP · { $moves ->
    [one] { $moves } move
   *[other] { $moves } moves
} · civilian
sandbox.lib.hall-of-fame = Sandbox · { $faction } · { $outcome } on turn { $round }
sandbox.lib.outcome-playing = Playing
sandbox.lib.outcome-victory = Victory
sandbox.lib.outcome-defeat = Defeat

## sandbox.lib: key help (Settings screen)
sandbox.lib.key-left-click = Left click
sandbox.lib.key-left-click-help = Select a unit or open a city
sandbox.lib.key-right-click = Right click
sandbox.lib.key-right-click-help = Move the unit there, or attack

## sandbox.screens
sandbox.screens.city-title = City
sandbox.screens.new-game-title = New sandbox game
sandbox.screens.sandbox-heading = Sandbox
sandbox.screens.about = A small 4X on the platform's kernel: four factions found cities, grow them on food and production and fight for every capital. The largest empire at turn 100 wins.
sandbox.screens.start-game = Start game
sandbox.screens.choose-city = Choose one of your cities on the map.
sandbox.screens.city-line = Population { $population } · { $food } food · { $production } production
sandbox.screens.producing = PRODUCING
sandbox.screens.producing-item = { $item } — { $stored } stored
sandbox.screens.producing-nothing = Nothing: choose what to build
sandbox.screens.build-next = BUILD NEXT
sandbox.screens.turns = { $n ->
    [one] { $n } turn
   *[other] { $n } turns
}
sandbox.screens.never = never
sandbox.screens.offer = { $name }   { $cost } · { $turns }
sandbox.screens.esc-hint = Esc returns to the map.

## sandbox.session
sandbox.session.not-a-save = Not a sandbox save ({ $detail })
sandbox.session.save-failed = The save does not load ({ $detail })
sandbox.session.top-food = { $amount } Food
sandbox.session.top-production = { $amount } Production
sandbox.session.top-cities = { $n ->
    [one] { $n } City
   *[other] { $n } Cities
}
