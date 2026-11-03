# Interfaz del juego: menú, ajustes, marco de la pantalla de juego, modo observador.

## shell
shell.turn = Turno
shell.turn.your-move = Su turno
shell.turn.not-your-move = No es su turno
shell.end-turn = Fin de turno
shell.end-turn-tip = Termine su turno; las demás naciones se mueven
shell.game-over = Fin de la partida
shell.others-moving = Los demás se mueven…
shell.empire = Imperio
shell.navigate = Navegar
shell.back = Atrás
shell.back-to-map = Volver al mapa
shell.world-map = Mapa del mundo
shell.menu = Menú
shell.settings = Ajustes
shell.grows = Crece
shell.stagnant = Estancada
shell.choose-production = Elegir producción

## shell.main-menu
shell.main-menu.resume = Continuar
shell.main-menu.new-game = Partida nueva
shell.main-menu.save-game = Guardar partida
shell.main-menu.load-game = Cargar partida
shell.main-menu.quit = Salir
shell.main-menu.report-bug = Informar de un error
shell.main-menu.your-world = SU MUNDO
shell.main-menu.explore = Explorar Rivermere
shell.main-menu.continue-journey = Continuar el viaje
shell.main-menu.gallery = Ilustraciones de las naciones
shell.main-menu.showcase-title = RIVERMERE
shell.main-menu.showcase-line = Un imperio cobra forma

## shell.report
shell.report.title = Informar de un error
shell.report.intro = El botón de abajo abre una incidencia de GitHub con el texto que ves aquí ya rellenado. El juego no envía nada: tú lees la página y pulsas «Submit». Tu nombre, tus carpetas y tu dirección de red no se incluyen.
shell.report.preview = Qué va a la incidencia
shell.report.hint = Primero guarda el archivo del informe y luego arrástralo a la incidencia: contiene estos datos y tu partida, para que los desarrolladores puedan reproducirla.
shell.report.open-issue = Abrir la página de la incidencia
shell.report.save-file = Guardar archivo del informe
shell.report.copy-path = Copiar ruta
shell.report.copied = Ruta copiada
shell.report.close = Cerrar
shell.report.saved-path = Guardado: { $place }
shell.report.saved-download = Descargado: { $place }. Arrástralo a la incidencia.
shell.report.cannot-write = No se pudo guardar el archivo del informe: { $error }
shell.report.crashed-title = Aurelia se ha detenido
shell.report.crashed-intro = El juego ha tenido un error y no puede continuar. Infórmalo: el botón abre una incidencia de GitHub con el texto de abajo, y no se envía nada hasta que pulses «Submit» allí. Recarga la página para volver a jugar.
shell.report.crashed-saved = Archivo del informe descargado. Arrástralo a la incidencia.
shell.report.reload = Recargar la página

## shell.saves
shell.saves.wait-for-turns = Espere a que las demás naciones terminen su turno
shell.saves.saved = Guardado en el turno { $turn }
shell.saves.loaded = Turno { $turn } cargado
shell.saves.nothing-saved = Todavía no hay ninguna partida guardada
shell.saves.loaded-world = Mundo cargado
shell.saves.other-module = Esta partida se guardó con las reglas { $saved }; este juego usa las reglas { $running }
shell.saves.unknown-module = Este archivo no indica con qué reglas se guardó
shell.saves.not-persistent = Las partidas no se conservarán: este navegador no permite el almacenamiento (¿ventana privada?)
shell.saves.load-title = Cargar partida
shell.saves.save-title = Guardar partida
shell.saves.close = Cerrar
shell.saves.empty = Aún no hay partidas guardadas
shell.saves.load = Cargar
shell.saves.delete = Eliminar
shell.saves.delete-ask = ¿Eliminar «{ $name }»?
shell.saves.delete-yes = Eliminar
shell.saves.cancel = Cancelar
shell.saves.deleted = Partida eliminada
shell.saves.not-in-list = esta partida no está en la lista
shell.saves.mark-auto = Auto
shell.saves.mark-quick = Rápido
shell.saves.name-quick = Guardado rápido
shell.saves.name-auto = Autoguardado
shell.saves.row-turn = Turno { $turn }
shell.saves.unreadable = Ilegible
shell.saves.reading = Leyendo…
shell.saves.problem-newer = Guardada con una versión más reciente del juego (formato { $format })
shell.saves.problem-older = Guardada con una versión anterior que este juego ya no lee (formato { $format })
shell.saves.problem-damaged = El archivo está dañado: { $detail }
shell.saves.age-now = Ahora mismo
shell.saves.age-minutes = { $n ->
    [one] hace { $n } minuto
   *[other] hace { $n } minutos
}
shell.saves.age-hours = { $n ->
    [one] hace { $n } hora
   *[other] hace { $n } horas
}
shell.saves.age-days = { $n ->
    [one] hace { $n } día
   *[other] hace { $n } días
}
shell.saves.age-date = { DATE($date) }
shell.saves.name-label = Nombre de la partida
shell.saves.name-replaces = Ya existe una partida con este nombre; se reemplazará
shell.saves.name-save = Guardar
shell.saves.name-replace = Reemplazar
shell.saves.default-name = { $civ }, turno { $turn }
shell.saves.default-name-plain = Turno { $turn }

## shell.options
shell.options.audio = Audio
shell.options.graphics = Gráficos
shell.options.game = Juego
shell.options.controls = Controles
shell.options.applied = Los cambios se aplican al instante y se guardan automáticamente.
shell.options.off = Desactivado
shell.options.on = Activado
shell.options.master-volume.label = Volumen general
shell.options.master-volume.help = Volumen global de la música y los efectos de sonido.
shell.options.music-volume.label = Volumen de la música
shell.options.music-volume.help = La música que acompaña a su nación.
shell.options.mute-music.label = Silenciar música
shell.options.mute-music.help = Silencia la música sin cambiar su volumen.
shell.options.effects-volume.label = Volumen de efectos
shell.options.effects-volume.help = Sonidos de la interfaz y eventos en el mapa.
shell.options.mute-effects.label = Silenciar efectos de sonido
shell.options.mute-effects.help = Silencia los efectos de sonido sin cambiar la música.
shell.options.audio-failed = No se pudieron reproducir algunos sonidos.
shell.options.window-mode.label = Modo de ventana
shell.options.window-mode.help = Elija una ventana o la pantalla completa.
shell.options.windowed = En ventana
shell.options.fullscreen = Pantalla completa
shell.options.window-size.label = Tamaño de ventana
shell.options.window-size.help = Tamaño de la ventana del juego.
shell.options.window-size-fullscreen = La pantalla completa usa la resolución de su pantalla.
shell.options.quality = Calidad gráfica
shell.options.quality-low.label = Baja
shell.options.quality-low.help = Menos detalle a distancia; animación del agua en pausa.
shell.options.quality-medium.label = Media
shell.options.quality-medium.help = Detalle equilibrado con agua y ríos animados.
shell.options.quality-high.label = Alta
shell.options.quality-high.help = Todo el detalle a cualquier distancia, con agua animada.
shell.options.render-scale.label = Escala de renderizado
shell.options.render-scale.help = Nitidez con la que se dibuja el mapa antes de ajustarlo a la ventana; un valor menor es más rápido en tarjetas gráficas lentas. La interfaz sigue nítida.
shell.options.auto = Auto
shell.options.anti-aliasing.label = Suavizado de bordes
shell.options.anti-aliasing.help = Suaviza los bordes dentados del mapa; desactivado es más rápido en tarjetas gráficas lentas.
shell.options.interface-scale.label = Escala de la interfaz
shell.options.interface-scale.help = Ajusta el tamaño del texto y los controles.
shell.options.brightness.label = Brillo de las ilustraciones
shell.options.brightness.help = Brillo de las ilustraciones en menús y pantallas.
shell.options.autosave.label = Autoguardado
shell.options.autosave.help = Guarda su progreso automáticamente al final de un turno.
shell.options.never = Nunca
shell.options.every-turn = Cada turno
shell.options.every-turns = { $n ->
    [one] Cada turno
    [many] Cada { $n } turnos
   *[other] Cada { $n } turnos
}
shell.options.saves-kept.label = Partidas que conservar
shell.options.saves-kept.help = Las partidas automáticas más antiguas se reemplazan al alcanzar este límite.
shell.options.other-turns.label = Turnos de las demás naciones
shell.options.other-turns.help = Rapidez con la que se muestran las acciones en el mapa.
shell.options.speed-normal = Normal
shell.options.speed-fast = Rápida
shell.options.speed-skip = Omitir
shell.options.reduced-motion.label = Movimiento reducido
shell.options.reduced-motion.help = Pausa las animaciones de marcha y de combate y el agua en movimiento; desactiva los desplazamientos de cámara.
shell.options.wonder-camera.label = Cámara de maravillas
shell.options.wonder-camera.help = Mueve la cámara hasta una maravilla terminada.
shell.options.wonder-camera-off = Desactivada mientras el movimiento reducido está activo.
shell.options.language.label = Idioma
shell.options.language.help = Idioma de los menús, las pantallas y los textos del juego. Cambia al instante; «Automático» sigue el idioma de su sistema.
shell.options.language-auto = Automático
shell.options.language-custom = paquete personalizado
shell.options.web-mode.label = Modo de interfaz
shell.options.web-mode.help = El modo móvil y tableta tiene controles más grandes y gestos táctiles; el modo ordenador es para ratón y teclado. La página se recarga y la partida en curso termina.
shell.options.web-mode.mobile = Móvil y tableta
shell.options.web-mode.desktop = Ordenador
shell.options.language-status.machine-drafted = Traducción automática: aún no la ha revisado un hablante nativo.
shell.options.language-status.reviewed = Traducción revisada.
shell.options.language-status.community = Traducción de la comunidad.
shell.options.language-status.test = Idioma de prueba para comprobar el diseño.
shell.options.controls-help = Arrastrar para desplazar · Rueda para acercar · Clic en una unidad o ciudad para seleccionarla

## shell.hud
shell.hud.settings-tip = Ajustes
shell.hud.center-capital = Centrar en la capital
shell.hud.toggle-grid = Mostrar u ocultar la cuadrícula hexagonal
shell.hud.toggle-labels = Mostrar u ocultar los nombres de ciudades
shell.hud.toggle-yields = Mostrar u ocultar los rendimientos de las casillas
shell.hud.minimap-tip = Clic para mover la cámara
shell.hud.orders-unavailable = Órdenes no disponibles
shell.hud.turn-unavailable = El avance de turnos no está disponible en esta versión.
shell.hud.units = Unidades
shell.hud.garrison = Guarnición
shell.hud.this-turn = Este turno
shell.hud.turns = { $n ->
    [one] { $n } turno
    [many] { $n } turnos
   *[other] { $n } turnos
}
shell.hud.captured = Capturada
shell.hud.destroyed = Destruida
shell.hud.health = Salud { $current }/{ $max }
shell.hud.strength = Fuerza { $attack } en ataque · { $defense } en defensa
shell.hud.ranged = A distancia { $strength } · alcance { $range }
shell.hud.moves = Movimiento { $moves }
shell.hud.sight = Visión { $sight }
shell.hud.population = { $name } · población { $population }
shell.hud.combat-strength = Fuerza de combate { $strength }
shell.hud.food = Comida { $stored }/{ $needed }
shell.hud.grows-in = { $n ->
    [one] crece en { $n } turno
    [many] crece en { $n } turnos
   *[other] crece en { $n } turnos
}
shell.hud.not-growing = no crece
shell.hud.producing = Produciendo { $name } { $progress }/{ $cost }
shell.hud.manage-city = Clic para gestionar la ciudad
shell.hud.hit-points = Puntos de vida { $current }/{ $max }
shell.hud.strike-ready = Arco: ataca a una unidad a 2 casillas o menos

## shell.observer
shell.observer.entry = Observador
shell.observer.entry-tip = Vea jugar a las naciones de la IA toda la partida: la vista propia de cada nación, sin órdenes
shell.observer.turn-of = Turno { $round } de { $limit }
shell.observer.turn = Turno { $round }
shell.observer.pause = Pausa
shell.observer.resume = Reanudar
shell.observer.step = Avanzar
shell.observer.paused = en pausa
shell.observer.speed-1 = ×0.25
shell.observer.speed-2 = ×0.5
shell.observer.speed-3 = ×1
shell.observer.speed-4 = ×2
shell.observer.speed-5 = ×4
shell.observer.speed-6 = ×8
shell.observer.speed-7 = ×16
shell.observer.speed-8 = Máx.
shell.observer.slower-tip = Más lento ([)
shell.observer.faster-tip = Más rápido (])
shell.observer.step-tip = Jugar un turno en pausa (N)
shell.observer.pause-tip = Pausar o reanudar (Espacio)
shell.observer.fallen = eliminada
shell.observer.grid-hint = Pase el cursor sobre una casilla para ver qué hay · haga clic en un panel o pulse 1-8 para ver una nación · R: todo el mapa
shell.observer.zoom-hint = Cursor: la casilla · clic en una ciudad o unidad · C: ciudades · R: todo el mapa · Tab o flechas: cambiar de nación · 1-8: nación · G: cuadrícula · Esc: volver
shell.observer.view-single = Una nación
shell.observer.view-grid = Cuadrícula
shell.observer.view-single-tip = Ver una nación a tamaño completo (G)
shell.observer.view-grid-tip = Volver a la cuadrícula de todas las naciones (G o Esc)
shell.observer.camera-follow = Seguir
shell.observer.camera-free = Cámara libre
shell.observer.camera-follow-tip = La cámara encuadra las ciudades y la visión de la nación (F)
shell.observer.camera-free-tip = Arrastre el mapa o gire la rueda: la cámara se queda donde la deje
shell.observer.watch-all = Observar todas
shell.observer.watch-one = Observar una
shell.observer.watch-tip = Vea jugar a la IA: la cuadrícula de todas las naciones, o una nación sola
shell.observer.events-title = Eventos recientes
shell.observer.no-events = Aún nada
shell.observer.event-line = R{ $round }: { $text }
shell.observer.ev-founded = Fundación de { $city }
shell.observer.ev-captured = { $city } capturada a { $nation }
shell.observer.ev-captured-plain = { $city } capturada
shell.observer.ev-lost = { $city } perdida ante { $nation }
shell.observer.ev-lost-plain = { $city } perdida
shell.observer.ev-war = Guerra con { $nation }
shell.observer.ev-peace = Paz con { $nation }
shell.observer.ev-learned = Aprendido: { $tech }
shell.observer.ev-fell = Eliminada
shell.observer.finale-title = Fin de la partida
shell.observer.winner = { $name } ganó
shell.observer.winners = { $names } ganaron juntos
shell.observer.nobody = Nadie ganó
shell.observer.finale-line = Turno { $round } · { $reason }
shell.observer.cannot-play = No se puede seguir jugando una partida observada
shell.observer.keep-playing = Un turno más
shell.observer.to-menu = Menú principal
shell.observer.final-table = CLASIFICACIÓN FINAL
shell.observer.no-save = No se puede guardar una partida observada
shell.observer.reveal = Todo el mapa
shell.observer.reveal-tip = Abre todo el mapa en cada vista: sin niebla, con cada ciudad y unidad tal como está ahora (R)
shell.observer.reveal-tag = Todo el mapa abierto · R: devolver la niebla
shell.observer.cities = Ciudades
shell.observer.cities-tip = Fichas de las ciudades de la nación: qué hay en cada una, sin órdenes (C)
shell.observer.city-place = Ciudad { $place } de { $count }
shell.observer.card-hint = Teclas izquierda y derecha o 1-9: otra ciudad · Esc: cerrar
shell.observer.card-close = Cerrar
shell.observer.card-close-tip = Cerrar la ficha (Esc)
shell.observer.card-prev-tip = La ciudad anterior (tecla izquierda)
shell.observer.card-next-tip = La ciudad siguiente (tecla derecha)
shell.observer.city-click = Clic para abrir la ficha de la ciudad
shell.observer.units-title = Unidades
shell.observer.unit-name = { $kind } ({ $nation })
shell.observer.unit-health = salud { $current } de { $max }
shell.observer.hp-value = { $current } de { $max }
shell.observer.unit-sight = visión { $sight }
shell.observer.sight-unknown = visión desconocida
shell.observer.unit-fortified = fortificada
shell.observer.sight-label = Visión
shell.observer.health-label = Salud
shell.observer.nation-label = Nación
shell.observer.unit-hint = Esc: soltar la selección
shell.observer.sight-note = El contorno muestra lo que ve
shell.observer.key-pause.keys = Espacio
shell.observer.key-pause.what = Pausar y reanudar
shell.observer.key-speed.keys = [ y ]
shell.observer.key-speed.what = Más lento y más rápido
shell.observer.key-step.keys = N
shell.observer.key-step.what = Jugar un turno en pausa
shell.observer.key-grid.keys = G
shell.observer.key-grid.what = La cuadrícula de todas las naciones, o una nación
shell.observer.key-tab.keys = Tab, Shift+Tab
shell.observer.key-tab.what = Otra nación (una nación a tamaño completo)
shell.observer.key-arrows.keys = Teclas izquierda y derecha
shell.observer.key-arrows.what = Otra nación; con una ficha de ciudad abierta, otra ciudad
shell.observer.key-numbers.keys = 1-8
shell.observer.key-numbers.what = Una nación por su número; con una ficha de ciudad abierta, una ciudad por su número
shell.observer.key-cities.keys = C
shell.observer.key-cities.what = Fichas de las ciudades de la nación
shell.observer.key-reveal.keys = R
shell.observer.key-reveal.what = Todo el mapa abierto, o devolver la niebla
shell.observer.key-follow.keys = F
shell.observer.key-follow.what = La cámara sigue el territorio de la nación
shell.observer.key-hover.keys = Pasar el cursor sobre una casilla
shell.observer.key-hover.what = Qué hay: terreno, rendimientos, mejora, recurso, propietario, unidades
shell.observer.key-click-city.keys = Clic en una ciudad
shell.observer.key-click-city.what = Su ficha: qué hay en ella, sin órdenes
shell.observer.key-click-unit.keys = Clic en una unidad
shell.observer.key-click-unit.what = Seleccionarla: contorno de su visión y sus detalles
shell.observer.key-escape.keys = Esc
shell.observer.key-escape.what = Cerrar la ficha, soltar la selección, volver a la cuadrícula

## Lands & Laws dice facción donde el juego base dice nación
shell.end-turn-tip@lal = Termine su turno; las demás facciones se mueven y después se cuenta la economía del turno
shell.main-menu.gallery@lal = Ilustraciones de las facciones
shell.saves.wait-for-turns@lal = Espere a que las demás facciones terminen su turno
shell.options.music-volume.help@lal = La música que acompaña a su facción.
shell.options.other-turns.label@lal = Turnos de las demás facciones
shell.observer.entry-tip@lal = Vea jugar a las facciones de la IA toda la partida: la vista propia de cada facción, sin órdenes
shell.observer.grid-hint@lal = Pase el cursor sobre una casilla para ver qué hay · haga clic en un panel o pulse 1-8 para ver una facción · R: todo el mapa
shell.observer.zoom-hint@lal = Cursor: la casilla · clic en una ciudad o unidad · C: ciudades · R: todo el mapa · Tab o flechas: cambiar de facción · 1-8: facción · G: cuadrícula · Esc: volver
shell.observer.view-single@lal = Una facción
shell.observer.view-single-tip@lal = Ver una facción a tamaño completo (G)
shell.observer.view-grid-tip@lal = Volver a la cuadrícula de todas las facciones (G o Esc)
shell.observer.camera-follow-tip@lal = La cámara encuadra las ciudades y la visión de la facción (F)
shell.observer.watch-tip@lal = Vea jugar a la IA: la cuadrícula de todas las facciones, o una facción sola
shell.observer.cities-tip@lal = Fichas de las ciudades de la facción: qué hay en cada una, sin órdenes (C)
shell.observer.nation-label@lal = Facción
shell.observer.key-grid.what@lal = La cuadrícula de todas las facciones, o una facción
shell.observer.key-tab.what@lal = Otra facción (una facción a tamaño completo)
shell.observer.key-arrows.what@lal = Otra facción; con una ficha de ciudad abierta, otra ciudad
shell.observer.key-numbers.what@lal = Una facción por su número; con una ficha de ciudad abierta, una ciudad por su número
shell.observer.key-cities.what@lal = Fichas de las ciudades de la facción
shell.observer.key-follow.what@lal = La cámara sigue el territorio de la facción

## Added after the first draft: cover, save drift notice, file errors, showcase sample
shell.cover.kicker = UN JUEGO DE ESTRATEGIA
shell.cover.tagline = Un mundo a la espera de ser escrito
shell.saves.drift = Guardado con otras reglas ({ $saved }); cargado con las reglas actuales ({ $current }); { $changed }
shell.saves.drift-changed = cambiado: { $parts }
shell.saves.drift-none = ninguna parte difiere
shell.saves.drift-unknown = la partida guardada no indica qué partes cambiaron
shell.errors.cannot-create = No se pudo crear { $path }: { $error }
shell.errors.cannot-read = No se pudo leer { $path }: { $error }
shell.errors.cannot-remove = No se pudo eliminar { $path }: { $error }
shell.errors.cannot-read-save = No se pudo leer la partida guardada: { $error }
shell.errors.cannot-save = No se pudo guardar: { $error }
shell.errors.storage-full = el almacenamiento del navegador está lleno; borra partidas antiguas o libera espacio para este sitio
shell.errors.cannot-rotate = No se pudieron renovar los guardados automáticos: { $error }
shell.errors.cannot-write-journal = No se pudo escribir el diario de órdenes: { $error }
shell.errors.cannot-save-settings = No se pudieron guardar los ajustes: { $error }
shell.errors.hall-open = No se pudo abrir el Salón de la Fama: { $error }
shell.errors.hall-write = No se pudo escribir en el Salón de la Fama: { $error }
shell.showcase.research = INVESTIGACIÓN
shell.showcase.research-name = Minería
shell.showcase.research-turns = 3 turnos
