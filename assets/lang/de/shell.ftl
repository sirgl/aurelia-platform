# Oberfläche des Spiels: Menü, Einstellungen, Spielrahmen, Beobachtermodus.

## shell
shell.turn = Zug
shell.turn.your-move = Sie sind am Zug
shell.turn.not-your-move = Sie sind nicht am Zug
shell.end-turn = Zug beenden
shell.end-turn-tip = Zug beenden; die anderen Nationen sind am Zug
shell.game-over = Spiel vorbei
shell.others-moving = Die anderen sind am Zug…
shell.empire = Reich
shell.navigate = Navigation
shell.back = Zurück
shell.back-to-map = Zur Karte
shell.world-map = Weltkarte
shell.menu = Menü
shell.settings = Einstellungen
shell.grows = Wächst
shell.stagnant = Stagniert
shell.choose-production = Produktion wählen

## shell.main-menu
shell.main-menu.resume = Fortsetzen
shell.main-menu.new-game = Neues Spiel
shell.main-menu.save-game = Spiel speichern
shell.main-menu.load-game = Spiel laden
shell.main-menu.quit = Beenden
shell.main-menu.report-bug = Fehler melden
shell.main-menu.your-world = IHRE WELT
shell.main-menu.explore = Rivermere erkunden
shell.main-menu.continue-journey = Reise fortsetzen
shell.main-menu.gallery = Bilder der Nationen
shell.main-menu.showcase-title = RIVERMERE
shell.main-menu.showcase-line = Ein Reich nimmt Gestalt an

## shell.report
shell.report.title = Fehler melden
shell.report.intro = Die Schaltfläche unten öffnet ein GitHub-Issue, in dem der Text von hier schon eingetragen ist. Das Spiel sendet nichts: Sie lesen die Seite und klicken selbst auf „Submit“. Ihr Name, Ihre Ordner und Ihre Netzwerkadresse sind nicht enthalten.
shell.report.preview = Das kommt ins Issue
shell.report.hint = Speichern Sie zuerst die Berichtsdatei und ziehen Sie sie dann ins Issue: Sie enthält diese Angaben und Ihr Spiel, damit die Entwickler es nachspielen können.
shell.report.open-issue = Issue-Seite öffnen
shell.report.save-file = Berichtsdatei speichern
shell.report.copy-path = Pfad kopieren
shell.report.copied = Pfad kopiert
shell.report.close = Schließen
shell.report.saved-path = Gespeichert: { $place }
shell.report.saved-download = Heruntergeladen: { $place }. Ziehen Sie die Datei ins Issue.
shell.report.cannot-write = Die Berichtsdatei konnte nicht gespeichert werden: { $error }
shell.report.crashed-title = Aurelia wurde angehalten
shell.report.crashed-intro = Im Spiel ist ein Fehler aufgetreten, es kann nicht weitergehen. Bitte melden Sie ihn: Die Schaltfläche öffnet ein GitHub-Issue mit dem Text unten, gesendet wird erst, wenn Sie dort auf „Submit“ klicken. Laden Sie die Seite neu, um weiterzuspielen.
shell.report.crashed-saved = Berichtsdatei heruntergeladen. Ziehen Sie sie ins Issue.
shell.report.reload = Seite neu laden

## shell.saves
shell.saves.wait-for-turns = Warten, bis die anderen Nationen ihren Zug beendet haben
shell.saves.saved = Gespeichert in Zug { $turn }
shell.saves.loaded = Zug { $turn } geladen
shell.saves.nothing-saved = Es gibt noch keinen Spielstand
shell.saves.loaded-world = Welt geladen
shell.saves.other-module = Dieser Spielstand wurde mit den Regeln { $saved } erstellt; dieses Spiel läuft mit den Regeln { $running }
shell.saves.unknown-module = Diese Datei nennt die Regeln nicht, mit denen sie gespeichert wurde
shell.saves.not-persistent = Spielstände bleiben nicht erhalten: Dieser Browser erlaubt keinen Speicher (privates Fenster?)
shell.saves.load-title = Spiel laden
shell.saves.save-title = Spiel speichern
shell.saves.close = Schließen
shell.saves.empty = Noch keine Spielstände vorhanden
shell.saves.load = Laden
shell.saves.delete = Löschen
shell.saves.delete-ask = „{ $name }“ löschen?
shell.saves.delete-yes = Löschen
shell.saves.cancel = Abbrechen
shell.saves.deleted = Spielstand gelöscht
shell.saves.not-in-list = dieser Spielstand steht nicht in der Liste
shell.saves.mark-auto = Auto
shell.saves.mark-quick = Schnell
shell.saves.name-quick = Schnellspeicherung
shell.saves.name-auto = Autospeicherung
shell.saves.row-turn = Zug { $turn }
shell.saves.unreadable = Nicht lesbar
shell.saves.reading = Lese…
shell.saves.problem-newer = Mit einer neueren Spielversion gespeichert (Format { $format })
shell.saves.problem-older = Mit einer älteren Version gespeichert, die dieses Spiel nicht mehr liest (Format { $format })
shell.saves.problem-damaged = Die Datei ist beschädigt: { $detail }
shell.saves.age-now = Gerade eben
shell.saves.age-minutes = { $n ->
    [one] vor { $n } Minute
   *[other] vor { $n } Minuten
}
shell.saves.age-hours = { $n ->
    [one] vor { $n } Stunde
   *[other] vor { $n } Stunden
}
shell.saves.age-days = { $n ->
    [one] vor { $n } Tag
   *[other] vor { $n } Tagen
}
shell.saves.age-date = { DATE($date) }
shell.saves.name-label = Name des Spielstands
shell.saves.name-replaces = Ein Spielstand mit diesem Namen existiert und wird ersetzt
shell.saves.name-save = Speichern
shell.saves.name-replace = Ersetzen
shell.saves.default-name = { $civ }, Zug { $turn }
shell.saves.default-name-plain = Zug { $turn }

## shell.options
shell.options.audio = Audio
shell.options.graphics = Grafik
shell.options.game = Spiel
shell.options.controls = Steuerung
shell.options.applied = Änderungen werden sofort übernommen und automatisch gespeichert.
shell.options.off = Aus
shell.options.on = Ein
shell.options.master-volume.label = Gesamtlautstärke
shell.options.master-volume.help = Gesamtlautstärke für Musik und Soundeffekte.
shell.options.music-volume.label = Musiklautstärke
shell.options.music-volume.help = Die Musik, die Ihre Nation begleitet.
shell.options.mute-music.label = Musik stummschalten
shell.options.mute-music.help = Schaltet die Musik stumm, ohne ihre Lautstärke zu ändern.
shell.options.effects-volume.label = Effektlautstärke
shell.options.effects-volume.help = Oberflächengeräusche und Ereignisse auf der Karte.
shell.options.mute-effects.label = Soundeffekte stummschalten
shell.options.mute-effects.help = Schaltet Soundeffekte stumm, ohne die Musik zu ändern.
shell.options.audio-failed = Einige Töne konnten nicht abgespielt werden.
shell.options.window-mode.label = Fenstermodus
shell.options.window-mode.help = Fenster oder Vollbild wählen.
shell.options.windowed = Fenster
shell.options.fullscreen = Vollbild
shell.options.window-size.label = Fenstergröße
shell.options.window-size.help = Größe des Spielfensters.
shell.options.window-size-fullscreen = Im Vollbild gilt die Auflösung Ihres Bildschirms.
shell.options.quality = Grafikqualität
shell.options.quality-low.label = Niedrig
shell.options.quality-low.help = Weniger Details in der Ferne; Wasseranimation pausiert.
shell.options.quality-medium.label = Mittel
shell.options.quality-medium.help = Ausgewogene Details mit animiertem Wasser und Flüssen.
shell.options.quality-high.label = Hoch
shell.options.quality-high.help = Volle Details in jeder Entfernung, Wasser animiert.
shell.options.render-scale.label = Renderauflösung
shell.options.render-scale.help = Wie scharf die Karte gezeichnet wird, bevor sie ins Fenster eingepasst wird; weniger ist auf langsameren Grafikkarten schneller. Die Oberfläche bleibt scharf.
shell.options.auto = Auto
shell.options.anti-aliasing.label = Kantenglättung
shell.options.anti-aliasing.help = Glättet gezackte Kanten auf der Karte; aus ist auf langsameren Grafikkarten schneller.
shell.options.interface-scale.label = Oberflächengröße
shell.options.interface-scale.help = Passt die Größe von Text und Bedienelementen an.
shell.options.brightness.label = Helligkeit der Bilder
shell.options.brightness.help = Helligkeit der Illustrationen in Menüs und Bildschirmen.
shell.options.autosave.label = Autospeichern
shell.options.autosave.help = Speichert Ihren Fortschritt automatisch am Ende eines Zugs.
shell.options.never = Nie
shell.options.every-turn = Jeder Zug
shell.options.every-turns = { $n ->
    [one] Jeder Zug
   *[other] Alle { $n } Züge
}
shell.options.saves-kept.label = Zu behaltende Spielstände
shell.options.saves-kept.help = Ältere automatische Spielstände werden ersetzt, wenn dieses Limit erreicht ist.
shell.options.other-turns.label = Züge der anderen Nationen
shell.options.other-turns.help = Wie schnell Aktionen auf der Karte gezeigt werden.
shell.options.speed-normal = Normal
shell.options.speed-fast = Schnell
shell.options.speed-skip = Überspringen
shell.options.reduced-motion.label = Weniger Bewegung
shell.options.reduced-motion.help = Pausiert Lauf- und Kampfanimationen sowie bewegtes Wasser; schaltet Kamerafahrten ab.
shell.options.wonder-camera.label = Wunderkamera
shell.options.wonder-camera.help = Bewegt die Kamera zu einem fertiggestellten Wunder.
shell.options.wonder-camera-off = Deaktiviert, solange „Weniger Bewegung“ aktiv ist.
shell.options.language.label = Sprache
shell.options.language.help = Sprache von Menüs, Bildschirmen und Spieltexten. Die Änderung gilt sofort; „Automatisch“ folgt Ihrem System.
shell.options.language-auto = Automatisch
shell.options.language-custom = eigenes Sprachpaket
shell.options.web-mode.label = Oberflächenmodus
shell.options.web-mode.help = Der Handy- und Tablet-Modus hat größere Schaltflächen und Touch-Gesten; der Desktop-Modus ist für Maus und Tastatur. Die Seite lädt neu und eine laufende Partie endet.
shell.options.web-mode.mobile = Handy und Tablet
shell.options.web-mode.desktop = Desktop
shell.options.language-status.machine-drafted = Maschinelle Übersetzung: Ein Muttersprachler hat sie noch nicht geprüft.
shell.options.language-status.reviewed = Geprüfte Übersetzung.
shell.options.language-status.community = Übersetzung der Community.
shell.options.language-status.test = Testsprache für Layoutprüfungen.
shell.options.controls-help = Ziehen zum Verschieben · Scrollen zum Zoomen · Klick auf eine Einheit oder Stadt wählt sie aus

## shell.hud
shell.hud.settings-tip = Einstellungen
shell.hud.center-capital = Auf Hauptstadt zentrieren
shell.hud.toggle-grid = Hex-Raster ein- oder ausblenden
shell.hud.toggle-labels = Stadtnamen ein- oder ausblenden
shell.hud.toggle-yields = Felderträge ein- oder ausblenden
shell.hud.minimap-tip = Klicken, um die Kamera zu bewegen
shell.hud.orders-unavailable = Befehle nicht verfügbar
shell.hud.turn-unavailable = Der Zugwechsel ist in dieser Version nicht verfügbar.
shell.hud.units = Einheiten
shell.hud.garrison = Garnison
shell.hud.this-turn = Dieser Zug
shell.hud.turns = { $n ->
    [one] { $n } Zug
   *[other] { $n } Züge
}
shell.hud.captured = Gefangen
shell.hud.destroyed = Zerstört
shell.hud.health = Gesundheit { $current }/{ $max }
shell.hud.strength = Stärke { $attack } im Angriff · { $defense } in der Verteidigung
shell.hud.ranged = Fernkampf { $strength } · Reichweite { $range }
shell.hud.moves = Bewegung { $moves }
shell.hud.sight = Sicht { $sight }
shell.hud.population = { $name } · Bevölkerung { $population }
shell.hud.combat-strength = Kampfstärke { $strength }
shell.hud.food = Nahrung { $stored }/{ $needed }
shell.hud.grows-in = { $n ->
    [one] wächst in { $n } Zug
   *[other] wächst in { $n } Züge
}
shell.hud.not-growing = wächst nicht
shell.hud.producing = Produziert: { $name } { $progress }/{ $cost }
shell.hud.manage-city = Klicken, um die Stadt zu verwalten
shell.hud.hit-points = Trefferpunkte { $current }/{ $max }
shell.hud.strike-ready = Bogen: greift eine Einheit im Umkreis von 2 Feldern an

## shell.observer
shell.observer.entry = Beobachter
shell.observer.entry-tip = Den KI-Nationen beim ganzen Spiel zusehen: die eigene Sicht jeder Nation, keine Befehle
shell.observer.turn-of = Zug { $round } von { $limit }
shell.observer.turn = Zug { $round }
shell.observer.pause = Pause
shell.observer.resume = Weiter
shell.observer.step = Schritt
shell.observer.paused = pausiert
shell.observer.speed-1 = ×0.25
shell.observer.speed-2 = ×0.5
shell.observer.speed-3 = ×1
shell.observer.speed-4 = ×2
shell.observer.speed-5 = ×4
shell.observer.speed-6 = ×8
shell.observer.speed-7 = ×16
shell.observer.speed-8 = Max
shell.observer.slower-tip = Langsamer ([)
shell.observer.faster-tip = Schneller (])
shell.observer.step-tip = Einen Zug spielen, während pausiert ist (N)
shell.observer.pause-tip = Pausieren oder fortsetzen (Leertaste)
shell.observer.fallen = untergegangen
shell.observer.grid-hint = Auf ein Feld zeigen, um zu sehen, was dort ist · Feld der Übersicht anklicken oder 1-8 drücken, um eine Nation zu beobachten · R: ganze Karte
shell.observer.zoom-hint = Zeigen: das Feld · Stadt oder Einheit anklicken · C: Städte · R: ganze Karte · Tab oder Pfeiltasten: Nation wechseln · 1-8: Nation · G: Raster · Esc: zurück
shell.observer.view-single = Eine Nation
shell.observer.view-grid = Raster
shell.observer.view-single-tip = Eine Nation in voller Größe beobachten (G)
shell.observer.view-grid-tip = Zurück zum Raster aller Nationen (G oder Esc)
shell.observer.camera-follow = Folgen
shell.observer.camera-free = Freie Kamera
shell.observer.camera-follow-tip = Die Kamera zeigt die Städte und das Sichtfeld der Nation (F)
shell.observer.camera-free-tip = Karte ziehen, Mausrad drehen: Die Kamera bleibt, wo sie zuletzt war
shell.observer.watch-all = Alle beobachten
shell.observer.watch-one = Eine beobachten
shell.observer.watch-tip = Der KI beim Spielen zusehen: das Raster aller Nationen oder eine Nation allein
shell.observer.events-title = Letzte Ereignisse
shell.observer.no-events = Noch nichts
shell.observer.event-line = R{ $round }: { $text }
shell.observer.ev-founded = { $city } gegründet
shell.observer.ev-captured = { $city } von { $nation } erobert
shell.observer.ev-captured-plain = { $city } erobert
shell.observer.ev-lost = { $city } an { $nation } verloren
shell.observer.ev-lost-plain = { $city } verloren
shell.observer.ev-war = Krieg mit { $nation }
shell.observer.ev-peace = Frieden mit { $nation }
shell.observer.ev-learned = { $tech } erforscht
shell.observer.ev-fell = Untergang
shell.observer.finale-title = Spiel beendet
shell.observer.winner = { $name } hat gewonnen
shell.observer.winners = { $names } haben gemeinsam gewonnen
shell.observer.nobody = Niemand hat gewonnen
shell.observer.finale-line = Zug { $round } · { $reason }
shell.observer.cannot-play = Ein beobachtetes Spiel kann nicht weitergespielt werden
shell.observer.keep-playing = Noch ein Zug
shell.observer.to-menu = Hauptmenü
shell.observer.final-table = ENDSTAND
shell.observer.no-save = Ein beobachtetes Spiel kann nicht gespeichert werden
shell.observer.reveal = Ganze Karte
shell.observer.reveal-tip = Deckt die ganze Karte in jeder Ansicht auf: kein Nebel, jede Stadt und Einheit so, wie sie jetzt ist (R)
shell.observer.reveal-tag = Ganze Karte offen · R: Nebel zurückholen
shell.observer.cities = Städte
shell.observer.cities-tip = Karten der Städte der Nation: was in jeder ist, keine Befehle (C)
shell.observer.city-place = Stadt { $place } von { $count }
shell.observer.card-hint = Pfeiltasten links und rechts oder 1-9: andere Stadt · Esc: schließen
shell.observer.card-close = Schließen
shell.observer.card-close-tip = Karte schließen (Esc)
shell.observer.card-prev-tip = Die vorige Stadt (Pfeil links)
shell.observer.card-next-tip = Die nächste Stadt (Pfeil rechts)
shell.observer.city-click = Klicken, um die Stadtkarte zu öffnen
shell.observer.units-title = Einheiten
shell.observer.unit-name = { $kind } ({ $nation })
shell.observer.unit-health = Gesundheit { $current } von { $max }
shell.observer.hp-value = { $current } von { $max }
shell.observer.unit-sight = Sicht { $sight }
shell.observer.sight-unknown = Sicht unbekannt
shell.observer.unit-fortified = befestigt
shell.observer.sight-label = Sicht
shell.observer.health-label = Gesundheit
shell.observer.nation-label = Nation
shell.observer.unit-hint = Esc: Auswahl aufheben
shell.observer.sight-note = Der Umriss zeigt, was die Einheit sieht
shell.observer.key-pause.keys = Leertaste
shell.observer.key-pause.what = Pausieren und fortsetzen
shell.observer.key-speed.keys = [ und ]
shell.observer.key-speed.what = Langsamer und schneller
shell.observer.key-step.keys = N
shell.observer.key-step.what = Einen Zug spielen, während pausiert ist
shell.observer.key-grid.keys = G
shell.observer.key-grid.what = Das Raster aller Nationen oder eine Nation
shell.observer.key-tab.keys = Tab, Shift+Tab
shell.observer.key-tab.what = Andere Nation (eine Nation in voller Größe)
shell.observer.key-arrows.keys = Pfeiltasten links und rechts
shell.observer.key-arrows.what = Andere Nation; bei geöffneter Stadtkarte eine andere Stadt
shell.observer.key-numbers.keys = 1-8
shell.observer.key-numbers.what = Eine Nation nach ihrer Nummer; bei geöffneter Stadtkarte eine Stadt nach ihrer Nummer
shell.observer.key-cities.keys = C
shell.observer.key-cities.what = Karten der Städte der Nation
shell.observer.key-reveal.keys = R
shell.observer.key-reveal.what = Die ganze Karte offen oder der Nebel zurück
shell.observer.key-follow.keys = F
shell.observer.key-follow.what = Die Kamera folgt dem Land der Nation
shell.observer.key-hover.keys = Auf ein Feld zeigen
shell.observer.key-hover.what = Was dort ist: Gelände, Erträge, Verbesserung, Ressource, Besitzer, Einheiten
shell.observer.key-click-city.keys = Stadt anklicken
shell.observer.key-click-city.what = Ihre Karte: was in ihr ist, keine Befehle
shell.observer.key-click-unit.keys = Einheit anklicken
shell.observer.key-click-unit.what = Auswählen: Umriss ihrer Sicht und ihre Details
shell.observer.key-escape.keys = Esc
shell.observer.key-escape.what = Karte schließen, Auswahl aufheben, zurück zum Raster

## Lands & Laws sagt Fraktion, wo das Grundspiel Nation sagt
shell.end-turn-tip@lal = Zug beenden; die anderen Fraktionen sind am Zug, danach wird die Wirtschaft des Zugs berechnet
shell.main-menu.gallery@lal = Bilder der Fraktionen
shell.saves.wait-for-turns@lal = Warten, bis die anderen Fraktionen ihren Zug beendet haben
shell.options.music-volume.help@lal = Die Musik, die Ihre Fraktion begleitet.
shell.options.other-turns.label@lal = Züge der anderen Fraktionen
shell.observer.entry-tip@lal = Den KI-Fraktionen beim ganzen Spiel zusehen: die eigene Sicht jeder Fraktion, keine Befehle
shell.observer.grid-hint@lal = Auf ein Feld zeigen, um zu sehen, was dort ist · Feld der Übersicht anklicken oder 1-8 drücken, um eine Fraktion zu beobachten · R: ganze Karte
shell.observer.zoom-hint@lal = Zeigen: das Feld · Stadt oder Einheit anklicken · C: Städte · R: ganze Karte · Tab oder Pfeiltasten: Fraktion wechseln · 1-8: Fraktion · G: Raster · Esc: zurück
shell.observer.view-single@lal = Eine Fraktion
shell.observer.view-single-tip@lal = Eine Fraktion in voller Größe beobachten (G)
shell.observer.view-grid-tip@lal = Zurück zum Raster aller Fraktionen (G oder Esc)
shell.observer.camera-follow-tip@lal = Die Kamera zeigt die Städte und das Sichtfeld der Fraktion (F)
shell.observer.watch-tip@lal = Der KI beim Spielen zusehen: das Raster aller Fraktionen oder eine Fraktion allein
shell.observer.cities-tip@lal = Karten der Städte der Fraktion: was in jeder ist, keine Befehle (C)
shell.observer.nation-label@lal = Fraktion
shell.observer.key-grid.what@lal = Das Raster aller Fraktionen oder eine Fraktion
shell.observer.key-tab.what@lal = Andere Fraktion (eine Fraktion in voller Größe)
shell.observer.key-arrows.what@lal = Andere Fraktion; bei geöffneter Stadtkarte eine andere Stadt
shell.observer.key-numbers.what@lal = Eine Fraktion nach ihrer Nummer; bei geöffneter Stadtkarte eine Stadt nach ihrer Nummer
shell.observer.key-cities.what@lal = Karten der Städte der Fraktion
shell.observer.key-follow.what@lal = Die Kamera folgt dem Land der Fraktion

## Added after the first draft: cover, save drift notice, file errors, showcase sample
shell.cover.kicker = EIN STRATEGIESPIEL
shell.cover.tagline = Eine Welt, die darauf wartet, geschrieben zu werden
shell.saves.drift = Mit anderen Regeln gespeichert ({ $saved }); mit den aktuellen Regeln geladen ({ $current }); { $changed }
shell.saves.drift-changed = geändert: { $parts }
shell.saves.drift-none = kein Teil unterscheidet sich
shell.saves.drift-unknown = der Spielstand nennt nicht, welche Teile sich geändert haben
shell.errors.cannot-create = { $path } konnte nicht erstellt werden: { $error }
shell.errors.cannot-read = { $path } konnte nicht gelesen werden: { $error }
shell.errors.cannot-remove = { $path } konnte nicht entfernt werden: { $error }
shell.errors.cannot-read-save = Der Spielstand konnte nicht gelesen werden: { $error }
shell.errors.cannot-save = Speichern nicht möglich: { $error }
shell.errors.storage-full = der Speicher des Browsers ist voll; lösche alte Spielstände oder schaffe Platz für diese Seite
shell.errors.cannot-rotate = Die automatischen Spielstände konnten nicht erneuert werden: { $error }
shell.errors.cannot-write-journal = Das Befehlsprotokoll konnte nicht geschrieben werden: { $error }
shell.errors.cannot-save-settings = Die Einstellungen konnten nicht gespeichert werden: { $error }
shell.errors.hall-open = Die Ruhmeshalle konnte nicht geöffnet werden: { $error }
shell.errors.hall-write = In die Ruhmeshalle konnte nicht geschrieben werden: { $error }
shell.showcase.research = FORSCHUNG
shell.showcase.research-name = Bergbau
shell.showcase.research-turns = 3 Züge
