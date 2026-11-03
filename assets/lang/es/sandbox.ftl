# Translation of sandbox.ftl into es. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

sandbox.lib.title = Sandbox
sandbox.lib.found-city = Fundar ciudad
sandbox.lib.match-summary = { $faction } · mundo de { $width } × { $height }
sandbox.lib.unit-line = { $hp } PV · { $moves ->
    [one] { $moves } movimiento
   *[other] { $moves } movimientos
}
sandbox.lib.unit-line-civilian = { $hp } PV · { $moves ->
    [one] { $moves } movimiento
   *[other] { $moves } movimientos
} · civil
sandbox.lib.hall-of-fame = Sandbox · { $faction } · { $outcome } en el turno { $round }
sandbox.lib.outcome-playing = En juego
sandbox.lib.outcome-victory = Victoria
sandbox.lib.outcome-defeat = Derrota
sandbox.lib.key-left-click = Clic izquierdo
sandbox.lib.key-left-click-help = Seleccionar una unidad o abrir una ciudad
sandbox.lib.key-right-click = Clic derecho
sandbox.lib.key-right-click-help = Mover la unidad allí, o atacar
sandbox.screens.city-title = Ciudad
sandbox.screens.new-game-title = Nueva partida sandbox
sandbox.screens.sandbox-heading = Sandbox
sandbox.screens.about = Un pequeño 4X sobre el núcleo de la plataforma: cuatro facciones fundan ciudades, las hacen crecer con comida y producción y luchan por cada capital. Gana el imperio más grande en el turno 100.
sandbox.screens.start-game = Empezar partida
sandbox.screens.choose-city = Elija una de sus ciudades en el mapa.
sandbox.screens.city-line = Población { $population } · { $food } de comida · { $production } de producción
sandbox.screens.producing = PRODUCIENDO
sandbox.screens.producing-item = { $item } — { $stored } acumulados
sandbox.screens.producing-nothing = Nada: elija qué construir
sandbox.screens.build-next = CONSTRUIR A CONTINUACIÓN
sandbox.screens.turns = { $n ->
    [one] { $n } turno
   *[other] { $n } turnos
}
sandbox.screens.never = nunca
sandbox.screens.offer = { $name }   { $cost } · { $turns }
sandbox.screens.esc-hint = Esc vuelve al mapa.
sandbox.session.not-a-save = No es una partida guardada de sandbox ({ $detail })
sandbox.session.save-failed = La partida guardada no se puede cargar ({ $detail })
sandbox.session.top-food = { $amount } de comida
sandbox.session.top-production = { $amount } de producción
sandbox.session.top-cities = { $n ->
    [one] { $n } ciudad
   *[other] { $n } ciudades
}
