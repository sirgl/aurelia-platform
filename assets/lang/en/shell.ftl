# The shell: menu, settings, HUD frame, observer. Source of truth: en.

## shell
shell.turn = Turn
# The player's own move is not the turn counter: the module HUDs ask these two for it.
shell.turn.your-move = Your move
shell.turn.not-your-move = It is not your turn
shell.end-turn = End turn
shell.end-turn-tip = End your turn; the other nations move
shell.game-over = Game over
shell.others-moving = Others are moving…
shell.empire = Empire
shell.navigate = Navigate
shell.back = Back
shell.back-to-map = Back to map
shell.world-map = World map
shell.menu = Menu
shell.settings = Settings
shell.grows = Grows
shell.stagnant = Stagnant
shell.choose-production = Choose production

shell.cover.kicker = A STRATEGY GAME
shell.cover.tagline = A world waiting to be written

## shell.main-menu
shell.main-menu.resume = Continue
shell.main-menu.new-game = New game
shell.main-menu.save-game = Save game
shell.main-menu.load-game = Load game
shell.main-menu.quit = Quit
shell.main-menu.report-bug = Report a bug
shell.main-menu.your-world = YOUR WORLD
shell.main-menu.explore = Explore Rivermere
shell.main-menu.continue-journey = Continue your journey
shell.main-menu.gallery = Nation artwork
shell.main-menu.showcase-title = RIVERMERE
shell.main-menu.showcase-line = An empire taking shape

## shell.report
shell.report.title = Report a bug
shell.report.intro = The button below opens a GitHub issue with the text you see here already filled in. Nothing is sent from the game: you read the page and press Submit yourself. Your name, your folders and your network address are not included.
shell.report.preview = What goes into the issue
shell.report.hint = First save the report file, then drag it into the issue: it holds these facts and your game, so the developers can replay it.
shell.report.open-issue = Open the issue page
shell.report.save-file = Save report file
shell.report.copy-path = Copy path
shell.report.copied = Path copied
shell.report.close = Close
shell.report.saved-path = Saved: { $place }
shell.report.saved-download = Downloaded: { $place }. Drag it into the issue.
shell.report.cannot-write = Cannot save the report file: { $error }
shell.report.crashed-title = Aurelia has stopped
shell.report.crashed-intro = The game hit an error and cannot go on. Please report it: the button opens a GitHub issue with the text below filled in, and nothing is sent until you press Submit there. Reload the page to play again.
shell.report.crashed-saved = Report file downloaded. Drag it into the issue.
shell.report.reload = Reload the page

## shell.saves
shell.saves.wait-for-turns = Wait until the other nations end their turns
shell.saves.saved = Saved on turn { $turn }
shell.saves.loaded = Loaded turn { $turn }
shell.saves.nothing-saved = There is no saved game yet
shell.saves.loaded-world = Loaded world
shell.saves.other-module = This save was made with the { $saved } rules; this game runs the { $running } rules
shell.saves.drift = Saved with other rules ({ $saved }); loaded under the current rules ({ $current }); { $changed }
shell.saves.drift-changed = changed: { $parts }
shell.saves.drift-none = no part differs
shell.saves.drift-unknown = the save does not say which parts changed
shell.saves.unknown-module = This file does not name the rules it was saved with
shell.saves.not-persistent = Saves will not persist: this browser does not allow storage (a private window?)
shell.saves.load-title = Load game
shell.saves.save-title = Save game
shell.saves.close = Close
shell.saves.empty = There are no saved games yet
shell.saves.load = Load
shell.saves.delete = Delete
shell.saves.delete-ask = Delete “{ $name }”?
shell.saves.delete-yes = Delete
shell.saves.cancel = Cancel
shell.saves.deleted = Save deleted
shell.saves.not-in-list = this save is not in the list
shell.saves.mark-auto = Auto
shell.saves.mark-quick = Quick
shell.saves.name-quick = Quick save
shell.saves.name-auto = Autosave
shell.saves.row-turn = Turn { $turn }
shell.saves.unreadable = Unreadable
shell.saves.reading = Reading…
shell.saves.problem-newer = Saved by a newer version of the game (format { $format })
shell.saves.problem-older = Saved by an older version this game no longer reads (format { $format })
shell.saves.problem-damaged = The file is damaged: { $detail }
shell.saves.age-now = Just now
shell.saves.age-minutes = { $n ->
    [one] { $n } minute ago
   *[other] { $n } minutes ago
}
shell.saves.age-hours = { $n ->
    [one] { $n } hour ago
   *[other] { $n } hours ago
}
shell.saves.age-days = { $n ->
    [one] { $n } day ago
   *[other] { $n } days ago
}
shell.saves.age-date = { DATE($date) }
shell.saves.name-label = Name of the save
shell.saves.name-replaces = A save with this name exists; it will be replaced
shell.saves.name-save = Save
shell.saves.name-replace = Replace
shell.saves.default-name = { $civ } turn { $turn }
shell.saves.default-name-plain = Turn { $turn }

## shell.options
shell.options.audio = Audio
shell.options.graphics = Graphics
shell.options.game = Game
shell.options.controls = Controls
shell.options.applied = Changes apply immediately and are saved automatically.
shell.options.off = Off
shell.options.on = On
shell.options.master-volume.label = Master volume
shell.options.master-volume.help = Overall volume for music and sound effects.
shell.options.music-volume.label = Music volume
shell.options.music-volume.help = Music accompanying your nation.
shell.options.mute-music.label = Mute music
shell.options.mute-music.help = Silence music while keeping its volume setting.
shell.options.effects-volume.label = Effects volume
shell.options.effects-volume.help = Interface sounds and events on the map.
shell.options.mute-effects.label = Mute sound effects
shell.options.mute-effects.help = Silence sound effects without changing music.
shell.options.audio-failed = Some audio could not be played.
shell.options.window-mode.label = Window mode
shell.options.window-mode.help = Choose a window or fill your display.
shell.options.windowed = Windowed
shell.options.fullscreen = Fullscreen
shell.options.window-size.label = Window size
shell.options.window-size.help = Size of the game window.
shell.options.window-size-fullscreen = Fullscreen uses your display's resolution.
shell.options.quality = Graphics quality
shell.options.quality-low.label = Low
shell.options.quality-low.help = Lower detail in the distance; water animation paused.
shell.options.quality-medium.label = Medium
shell.options.quality-medium.help = Balanced detail with animated water and rivers.
shell.options.quality-high.label = High
shell.options.quality-high.help = Full detail at every distance with animated water.
shell.options.render-scale.label = Render scale
shell.options.render-scale.help = How sharp the map is drawn before it fits the window; less is faster on slower graphics cards. The interface stays sharp.
shell.options.auto = Auto
shell.options.anti-aliasing.label = Anti-aliasing
shell.options.anti-aliasing.help = Smooth jagged edges on the map; off is faster on slower graphics cards.
shell.options.interface-scale.label = Interface scale
shell.options.interface-scale.help = Adjust the size of text and controls.
shell.options.brightness.label = Artwork brightness
shell.options.brightness.help = Brightness of illustrations in menus and screens.
shell.options.autosave.label = Autosave
shell.options.autosave.help = Save your progress automatically at the end of a turn.
shell.options.never = Never
shell.options.every-turn = Every turn
shell.options.every-turns = { $n ->
    [one] Every { $n } turn
   *[other] Every { $n } turns
}
shell.options.saves-kept.label = Saves to keep
shell.options.saves-kept.help = Older automatic saves are replaced when this limit is reached.
shell.options.other-turns.label = Other nations' turns
shell.options.other-turns.help = How quickly actions are shown on the map.
shell.options.speed-normal = Normal
shell.options.speed-fast = Fast
shell.options.speed-skip = Skip
shell.options.reduced-motion.label = Reduced motion
shell.options.reduced-motion.help = Pause walking and combat animations and moving water; disable camera glides.
shell.options.wonder-camera.label = Wonder camera
shell.options.wonder-camera.help = Move the camera to a completed wonder.
shell.options.wonder-camera-off = Disabled while reduced motion is on.
shell.options.language.label = Language
shell.options.language.help = Language of menus, screens and game texts. It changes at once; “Automatic” follows your system.
shell.options.language-auto = Automatic
shell.options.language-custom = custom bundle
shell.options.web-mode.label = Interface mode
shell.options.web-mode.help = Phone and tablet mode has larger controls and touch gestures; desktop mode is for a mouse and keyboard. The page reloads and a game in progress ends.
shell.options.web-mode.mobile = Phone and tablet
shell.options.web-mode.desktop = Desktop
shell.options.language-status.machine-drafted = Machine-drafted translation: a native speaker has not reviewed it yet.
shell.options.language-status.reviewed = Reviewed translation.
shell.options.language-status.community = Community translation.
shell.options.language-status.test = Test language for layout checks.
shell.options.controls-help = Drag to pan · Scroll to zoom · Click a unit or city to select it

## shell.hud
shell.hud.settings-tip = Settings
shell.hud.center-capital = Center on capital
shell.hud.toggle-grid = Toggle hex grid
shell.hud.toggle-labels = Toggle city labels
shell.hud.toggle-yields = Toggle tile yields
shell.hud.minimap-tip = Click to move the camera
shell.hud.orders-unavailable = Orders unavailable
shell.hud.turn-unavailable = Turn progression is not available in this build.
shell.hud.units = Units
shell.hud.garrison = Garrison
shell.hud.this-turn = This turn
shell.hud.turns = { $n ->
    [one] { $n } turn
   *[other] { $n } turns
}
shell.hud.captured = Captured
shell.hud.destroyed = Destroyed
shell.hud.health = Health { $current }/{ $max }
shell.hud.strength = Strength { $attack } attacking · { $defense } defending
shell.hud.ranged = Ranged { $strength } · range { $range }
shell.hud.moves = Moves { $moves }
shell.hud.sight = Sight { $sight }
shell.hud.population = { $name } · population { $population }
shell.hud.combat-strength = Combat strength { $strength }
shell.hud.food = Food { $stored }/{ $needed }
shell.hud.grows-in = { $n ->
    [one] grows in { $n } turn
   *[other] grows in { $n } turns
}
shell.hud.not-growing = not growing
shell.hud.producing = Producing { $name } { $progress }/{ $cost }
shell.hud.manage-city = Click to manage the city
shell.hud.hit-points = Hit points { $current }/{ $max }
shell.hud.strike-ready = Bow: strike a unit within 2 tiles

## shell.observer
shell.observer.entry = Observer
shell.observer.entry-tip = Watch the AI nations play the whole match: every nation's own view, no orders
shell.observer.turn-of = Turn { $round } of { $limit }
shell.observer.turn = Turn { $round }
shell.observer.pause = Pause
shell.observer.resume = Resume
shell.observer.step = Step
shell.observer.paused = paused
shell.observer.speed-1 = ×0.25
shell.observer.speed-2 = ×0.5
shell.observer.speed-3 = ×1
shell.observer.speed-4 = ×2
shell.observer.speed-5 = ×4
shell.observer.speed-6 = ×8
shell.observer.speed-7 = ×16
shell.observer.speed-8 = Max
shell.observer.slower-tip = Slower ([)
shell.observer.faster-tip = Faster (])
shell.observer.step-tip = Play one turn while paused (N)
shell.observer.pause-tip = Pause or resume (Space)
shell.observer.fallen = fallen
shell.observer.grid-hint = Hover over a tile to see what is there · click a panel or press 1-8 to watch one nation · R: whole map
shell.observer.zoom-hint = Hover: the tile · click a city or a unit · C: cities · R: whole map · Tab or arrow keys: switch nation · 1-8: nation · G: grid · Esc: back
shell.observer.view-single = One nation
shell.observer.view-grid = Grid
shell.observer.view-single-tip = Watch one nation at full size (G)
shell.observer.view-grid-tip = Back to the grid of every nation (G or Esc)
shell.observer.camera-follow = Follow
shell.observer.camera-free = Free camera
shell.observer.camera-follow-tip = The camera frames the nation's cities and sight (F)
shell.observer.camera-free-tip = Drag the map, turn the wheel: the camera stays where you leave it
shell.observer.watch-all = Observe all
shell.observer.watch-one = Observe one
shell.observer.watch-tip = Watch the AI play: the grid of every nation, or one nation alone
shell.observer.events-title = Recent events
shell.observer.no-events = Nothing yet
shell.observer.event-line = T{ $round }: { $text }
shell.observer.ev-founded = Founded { $city }
shell.observer.ev-captured = Captured { $city } from { $nation }
shell.observer.ev-captured-plain = Captured { $city }
shell.observer.ev-lost = Lost { $city } to { $nation }
shell.observer.ev-lost-plain = Lost { $city }
shell.observer.ev-war = War with { $nation }
shell.observer.ev-peace = Peace with { $nation }
shell.observer.ev-learned = Learned { $tech }
shell.observer.ev-fell = Fell
shell.observer.finale-title = Match over
shell.observer.winner = { $name } won
shell.observer.winners = { $names } won together
shell.observer.nobody = Nobody won
shell.observer.finale-line = Turn { $round } · { $reason }
shell.observer.cannot-play = An observed match cannot be played on
shell.observer.keep-playing = One more turn
shell.observer.to-menu = Main menu
shell.observer.final-table = FINAL STANDINGS
shell.observer.no-save = An observed match cannot be saved
shell.observer.reveal = Whole map
shell.observer.reveal-tip = Open the whole map in every view: no fog, every city and unit as it is now (R)
shell.observer.reveal-tag = Whole map open · R: bring the fog back
shell.observer.cities = Cities
shell.observer.cities-tip = Cards of the nation's cities: what is in each, no orders (C)
shell.observer.city-place = City { $place } of { $count }
shell.observer.card-hint = Left and right keys or 1-9: another city · Esc: close
shell.observer.card-close = Close
shell.observer.card-close-tip = Close the card (Esc)
shell.observer.card-prev-tip = The previous city (left key)
shell.observer.card-next-tip = The next city (right key)
shell.observer.city-click = Click to open the city card
shell.observer.units-title = Units
shell.observer.unit-name = { $kind } ({ $nation })
shell.observer.unit-health = health { $current } of { $max }
shell.observer.hp-value = { $current } of { $max }
shell.observer.unit-sight = sight { $sight }
shell.observer.sight-unknown = sight not known
shell.observer.unit-fortified = fortified
shell.observer.sight-label = Sight
shell.observer.health-label = Health
shell.observer.nation-label = Nation
shell.observer.unit-hint = Esc: drop the choice
shell.observer.sight-note = The outline shows what it sees
shell.observer.key-pause.keys = Space
shell.observer.key-pause.what = Pause and resume
shell.observer.key-speed.keys = [ and ]
shell.observer.key-speed.what = Slower and faster
shell.observer.key-step.keys = N
shell.observer.key-step.what = Play one turn while paused
shell.observer.key-grid.keys = G
shell.observer.key-grid.what = The grid of every nation, or one nation
shell.observer.key-tab.keys = Tab, Shift+Tab
shell.observer.key-tab.what = Another nation (one nation at full size)
shell.observer.key-arrows.keys = Left and right keys
shell.observer.key-arrows.what = Another nation; with a city card open, another city
shell.observer.key-numbers.keys = 1-8
shell.observer.key-numbers.what = A nation by its number; with a city card open, a city by its number
shell.observer.key-cities.keys = C
shell.observer.key-cities.what = Cards of the nation's cities
shell.observer.key-reveal.keys = R
shell.observer.key-reveal.what = The whole map open, or the fog back
shell.observer.key-follow.keys = F
shell.observer.key-follow.what = The camera follows the nation's land
shell.observer.key-hover.keys = Hover over a tile
shell.observer.key-hover.what = What is there: land, yields, improvement, resource, owner, units
shell.observer.key-click-city.keys = Click a city
shell.observer.key-click-city.what = Its card: what is in it, no orders
shell.observer.key-click-unit.keys = Click a unit
shell.observer.key-click-unit.what = Choose it: outline of its sight and its details
shell.observer.key-escape.keys = Esc
shell.observer.key-escape.what = Close the card, drop the choice, back to the grid

## Lands & Laws says faction where the shell says nation
shell.end-turn-tip@lal = End your turn; the other factions move, then the turn's economy is counted
shell.main-menu.gallery@lal = Faction artwork
shell.saves.wait-for-turns@lal = Wait until the other factions end their turns
shell.options.music-volume.help@lal = Music accompanying your faction.
shell.options.other-turns.label@lal = Other factions' turns
shell.observer.entry-tip@lal = Watch the AI factions play the whole match: every faction's own view, no orders
shell.observer.grid-hint@lal = Hover over a tile to see what is there · click a panel or press 1-8 to watch one faction · R: whole map
shell.observer.zoom-hint@lal = Hover: the tile · click a city or a unit · C: cities · R: whole map · Tab or arrow keys: switch faction · 1-8: faction · G: grid · Esc: back
shell.observer.view-single@lal = One faction
shell.observer.view-single-tip@lal = Watch one faction at full size (G)
shell.observer.view-grid-tip@lal = Back to the grid of every faction (G or Esc)
shell.observer.camera-follow-tip@lal = The camera frames the faction's cities and sight (F)
shell.observer.watch-tip@lal = Watch the AI play: the grid of every faction, or one faction alone
shell.observer.cities-tip@lal = Cards of the faction's cities: what is in each, no orders (C)
shell.observer.nation-label@lal = Faction
shell.observer.key-grid.what@lal = The grid of every faction, or one faction
shell.observer.key-tab.what@lal = Another faction (one faction at full size)
shell.observer.key-arrows.what@lal = Another faction; with a city card open, another city
shell.observer.key-numbers.what@lal = A faction by its number; with a city card open, a city by its number
shell.observer.key-cities.what@lal = Cards of the faction's cities
shell.observer.key-follow.what@lal = The camera follows the faction's land

## shell.errors: what the player is told when a file cannot be read or written ($error is the system's text)
shell.errors.cannot-create = Cannot create { $path }: { $error }
shell.errors.cannot-read = Cannot read { $path }: { $error }
shell.errors.cannot-remove = Cannot remove { $path }: { $error }
shell.errors.cannot-read-save = Cannot read the save: { $error }
shell.errors.cannot-save = Cannot save: { $error }
shell.errors.storage-full = the browser's storage is full; delete old saves or free space for this site
shell.errors.cannot-rotate = Cannot rotate autosaves: { $error }
shell.errors.cannot-write-journal = Cannot write the command journal: { $error }
shell.errors.cannot-save-settings = Cannot save settings: { $error }
shell.errors.hall-open = Cannot open the Hall of Fame: { $error }
shell.errors.hall-write = Cannot write the Hall of Fame: { $error }

## shell.showcase: the research sample of the showcase
shell.showcase.research = RESEARCH
shell.showcase.research-name = Mining
shell.showcase.research-turns = 3 turns
