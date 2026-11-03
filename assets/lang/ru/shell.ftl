# Оболочка: меню, настройки, рамка HUD, наблюдатель.

## shell
shell.turn = Ход
shell.turn.your-move = Ваш ход
shell.turn.not-your-move = Сейчас не ваш ход
shell.end-turn = Конец хода
shell.end-turn-tip = Завершить ход: ходят другие державы
shell.game-over = Партия окончена
shell.others-moving = Ходят другие…
shell.empire = Держава
shell.navigate = Разделы
shell.back = Назад
shell.back-to-map = К карте
shell.world-map = Карта мира
shell.menu = Меню
shell.settings = Настройки
shell.grows = Рост
shell.stagnant = Без роста
shell.choose-production = Выберите производство

shell.cover.kicker = СТРАТЕГИЧЕСКАЯ ИГРА
shell.cover.tagline = Мир, который ещё предстоит написать

## shell.main-menu
shell.main-menu.resume = Продолжить
shell.main-menu.new-game = Новая партия
shell.main-menu.save-game = Сохранить
shell.main-menu.load-game = Загрузить
shell.main-menu.quit = Выйти
shell.main-menu.report-bug = Сообщить об ошибке
shell.main-menu.your-world = ВАШ МИР
shell.main-menu.explore = Осмотреть Ривермир
shell.main-menu.continue-journey = Продолжить путь
shell.main-menu.gallery = Галерея держав
shell.main-menu.showcase-title = РИВЕРМИР
shell.main-menu.showcase-line = Держава обретает форму

## shell.report
shell.report.title = Сообщить об ошибке
shell.report.intro = Кнопка ниже открывает на GitHub заявку с уже заполненным текстом, который вы видите здесь. Игра ничего не отправляет: вы сами читаете страницу и нажимаете «Submit». Ваше имя, ваши папки и сетевой адрес в текст не попадают.
shell.report.preview = Что попадёт в заявку
shell.report.hint = Сначала сохраните файл отчёта, потом перетащите его в заявку: в нём эти сведения и ваша игра, разработчики смогут её воспроизвести.
shell.report.open-issue = Открыть страницу заявки
shell.report.save-file = Сохранить файл отчёта
shell.report.copy-path = Копировать путь
shell.report.copied = Путь скопирован
shell.report.close = Закрыть
shell.report.saved-path = Сохранено: { $place }
shell.report.saved-download = Скачано: { $place }. Перетащите файл в заявку.
shell.report.cannot-write = Не удалось сохранить файл отчёта: { $error }
shell.report.crashed-title = Aurelia остановилась
shell.report.crashed-intro = В игре произошла ошибка, продолжить нельзя. Сообщите о ней: кнопка откроет на GitHub заявку с заполненным текстом ниже, ничего не отправляется, пока вы сами не нажмёте «Submit». Чтобы играть дальше, перезагрузите страницу.
shell.report.crashed-saved = Файл отчёта скачан. Перетащите его в заявку.
shell.report.reload = Перезагрузить страницу

## shell.saves
shell.saves.wait-for-turns = Подождите, пока другие державы закончат ход
shell.saves.saved = Сохранено на ходу { $turn }
shell.saves.loaded = Загружен ход { $turn }
shell.saves.nothing-saved = Сохранённых партий пока нет
shell.saves.loaded-world = Загруженный мир
shell.saves.other-module = Это сохранение сделано по правилам «{ $saved }», а игра идёт по правилам «{ $running }»
shell.saves.drift = Сохранено по другим правилам ({ $saved }); загружено по текущим ({ $current }); { $changed }
shell.saves.drift-changed = изменились: { $parts }
shell.saves.drift-none = различий в частях нет
shell.saves.drift-unknown = в сохранении не сказано, какие части изменились
shell.saves.unknown-module = В файле не указано, по каким правилам он сохранён
shell.saves.not-persistent = Сохранения не сохранятся: браузер не разрешает хранение данных (приватное окно?)
shell.saves.load-title = Загрузить игру
shell.saves.save-title = Сохранить игру
shell.saves.close = Закрыть
shell.saves.empty = Сохранённых партий пока нет
shell.saves.load = Загрузить
shell.saves.delete = Удалить
shell.saves.delete-ask = Удалить «{ $name }»?
shell.saves.delete-yes = Удалить
shell.saves.cancel = Отмена
shell.saves.deleted = Сохранение удалено
shell.saves.not-in-list = этого сохранения нет в списке
shell.saves.mark-auto = Авто
shell.saves.mark-quick = Быстрое
shell.saves.name-quick = Быстрое сохранение
shell.saves.name-auto = Автосохранение
shell.saves.row-turn = Ход { $turn }
shell.saves.unreadable = Не читается
shell.saves.reading = Чтение…
shell.saves.problem-newer = Сохранено более новой версией игры (формат { $format })
shell.saves.problem-older = Сохранено старой версией, которую игра больше не читает (формат { $format })
shell.saves.problem-damaged = Файл повреждён: { $detail }
shell.saves.age-now = Только что
shell.saves.age-minutes = { $n ->
    [one] { $n } минуту назад
    [few] { $n } минуты назад
    [many] { $n } минут назад
   *[other] { $n } минуты назад
}
shell.saves.age-hours = { $n ->
    [one] { $n } час назад
    [few] { $n } часа назад
    [many] { $n } часов назад
   *[other] { $n } часа назад
}
shell.saves.age-days = { $n ->
    [one] { $n } день назад
    [few] { $n } дня назад
    [many] { $n } дней назад
   *[other] { $n } дня назад
}
shell.saves.age-date = { DATE($date) }
shell.saves.name-label = Название сохранения
shell.saves.name-replaces = Сохранение с таким названием уже есть, оно будет заменено
shell.saves.name-save = Сохранить
shell.saves.name-replace = Заменить
shell.saves.default-name = { $civ }, ход { $turn }
shell.saves.default-name-plain = Ход { $turn }

## shell.options
shell.options.audio = Звук
shell.options.graphics = Графика
shell.options.game = Игра
shell.options.controls = Управление
shell.options.applied = Изменения действуют сразу и сохраняются сами.
shell.options.off = Выкл.
shell.options.on = Вкл.
shell.options.master-volume.label = Общая громкость
shell.options.master-volume.help = Громкость музыки и звуковых эффектов вместе.
shell.options.music-volume.label = Громкость музыки
shell.options.music-volume.help = Музыка, которая сопровождает партию.
shell.options.mute-music.label = Без музыки
shell.options.mute-music.help = Выключить музыку, сохранив её громкость.
shell.options.effects-volume.label = Громкость эффектов
shell.options.effects-volume.help = Звуки интерфейса и событий на карте.
shell.options.mute-effects.label = Без эффектов
shell.options.mute-effects.help = Выключить звуковые эффекты, не трогая музыку.
shell.options.audio-failed = Часть звуков не удалось воспроизвести.
shell.options.window-mode.label = Режим окна
shell.options.window-mode.help = Окно или весь экран.
shell.options.windowed = В окне
shell.options.fullscreen = Весь экран
shell.options.window-size.label = Размер окна
shell.options.window-size.help = Размер окна игры.
shell.options.window-size-fullscreen = На весь экран берётся разрешение дисплея.
shell.options.quality = Качество графики
shell.options.quality-low.label = Низкое
shell.options.quality-low.help = Меньше деталей вдали; вода не движется.
shell.options.quality-medium.label = Среднее
shell.options.quality-medium.help = Детали в меру, вода и реки движутся.
shell.options.quality-high.label = Высокое
shell.options.quality-high.help = Все детали на любом расстоянии, вода движется.
shell.options.render-scale.label = Масштаб отрисовки
shell.options.render-scale.help = Насколько чётко рисуется карта до подгонки под окно; меньше — быстрее на слабых видеокартах. Интерфейс остаётся чётким.
shell.options.auto = Авто
shell.options.anti-aliasing.label = Сглаживание
shell.options.anti-aliasing.help = Сглаживает рваные края на карте; без него быстрее на слабых видеокартах.
shell.options.interface-scale.label = Масштаб интерфейса
shell.options.interface-scale.help = Размер текста и кнопок.
shell.options.brightness.label = Яркость иллюстраций
shell.options.brightness.help = Яркость картин в меню и на экранах.
shell.options.autosave.label = Автосохранение
shell.options.autosave.help = Сохранять партию в конце хода.
shell.options.never = Никогда
shell.options.every-turn = Каждый ход
shell.options.every-turns = { $n ->
    [one] Раз в { $n } ход
    [few] Раз в { $n } хода
    [many] Раз в { $n } ходов
   *[other] Раз в { $n } хода
}
shell.options.saves-kept.label = Хранить сохранений
shell.options.saves-kept.help = Старые автосохранения заменяются, когда их больше этого числа.
shell.options.other-turns.label = Ходы других держав
shell.options.other-turns.help = Как быстро их действия показываются на карте.
shell.options.speed-normal = Обычно
shell.options.speed-fast = Быстро
shell.options.speed-skip = Пропускать
shell.options.reduced-motion.label = Меньше движения
shell.options.reduced-motion.help = Остановить анимации ходьбы, боя и воды; камера не перелетает.
shell.options.wonder-camera.label = Камера к чудесам
shell.options.wonder-camera.help = Показывать готовое великое строение.
shell.options.wonder-camera-off = Выключено, пока включено «Меньше движения».
shell.options.language.label = Язык
shell.options.language.help = Язык меню, экранов и текстов игры. Меняется сразу; «Автоматически» берёт язык системы.
shell.options.language-auto = Автоматически
shell.options.language-custom = свой пакет
shell.options.web-mode.label = Режим интерфейса
shell.options.web-mode.help = Режим для телефона и планшета: крупные кнопки и жесты; режим для компьютера: мышь и клавиатура. Страница перезагрузится, текущая партия закончится.
shell.options.web-mode.mobile = Телефон и планшет
shell.options.web-mode.desktop = Компьютер
shell.options.language-status.machine-drafted = Машинный перевод: носитель языка его ещё не проверял.
shell.options.language-status.reviewed = Проверенный перевод.
shell.options.language-status.community = Перевод сообщества.
shell.options.language-status.test = Тестовый язык для проверки вёрстки.
shell.options.controls-help = Перетаскивание — сдвиг карты · колесо — масштаб · щелчок — выбрать отряд или город

## shell.hud
shell.hud.settings-tip = Настройки
shell.hud.center-capital = К столице
shell.hud.toggle-grid = Сетка гексов
shell.hud.toggle-labels = Подписи городов
shell.hud.toggle-yields = Выход клеток
shell.hud.minimap-tip = Щёлкните, чтобы перенести камеру
shell.hud.orders-unavailable = Приказы недоступны
shell.hud.turn-unavailable = В этой сборке ходы недоступны.
shell.hud.units = Отряды
shell.hud.garrison = Гарнизон
shell.hud.this-turn = В этот ход
shell.hud.turns = { $n ->
    [one] { $n } ход
    [few] { $n } хода
    [many] { $n } ходов
   *[other] { $n } хода
}
shell.hud.captured = Захвачен
shell.hud.destroyed = Уничтожен
shell.hud.health = Прочность { $current }/{ $max }
shell.hud.strength = Сила { $attack } в атаке · { $defense } в обороне
shell.hud.ranged = Дальний бой { $strength } · дальность { $range }
shell.hud.moves = Ходы { $moves }
shell.hud.sight = Обзор { $sight }
shell.hud.population = { $name } · население { $population }
shell.hud.combat-strength = Сила обороны { $strength }
shell.hud.food = Еда { $stored }/{ $needed }
shell.hud.grows-in = { $n ->
    [one] рост через { $n } ход
    [few] рост через { $n } хода
    [many] рост через { $n } ходов
   *[other] рост через { $n } хода
}
shell.hud.not-growing = без роста
shell.hud.producing = Строится { $name } { $progress }/{ $cost }
shell.hud.manage-city = Щёлкните, чтобы открыть город
shell.hud.hit-points = Здоровье { $current }/{ $max }
shell.hud.strike-ready = Лук: ударить по отряду в двух клетках

## shell.observer
shell.observer.entry = Наблюдатель
shell.observer.entry-tip = Смотреть, как державы под управлением ИИ играют всю партию: у каждой свой вид, приказов нет
shell.observer.turn-of = Ход { $round } из { $limit }
shell.observer.turn = Ход { $round }
shell.observer.pause = Пауза
shell.observer.resume = Пуск
shell.observer.step = Шаг
shell.observer.paused = пауза
shell.observer.speed-1 = ×0,25
shell.observer.speed-2 = ×0,5
shell.observer.speed-3 = ×1
shell.observer.speed-4 = ×2
shell.observer.speed-5 = ×4
shell.observer.speed-6 = ×8
shell.observer.speed-7 = ×16
shell.observer.speed-8 = Макс
shell.observer.slower-tip = Медленнее ([)
shell.observer.faster-tip = Быстрее (])
shell.observer.step-tip = Сыграть один ход на паузе (N)
shell.observer.pause-tip = Пауза или пуск (Пробел)
shell.observer.fallen = пала
shell.observer.grid-hint = Наведите курсор на клетку, чтобы увидеть, что на ней · щёлкните по панели или нажмите 1–8, чтобы смотреть одну державу · R: вся карта
shell.observer.zoom-hint = Наведение: клетка · щелчок по городу или отряду · C: города · R: вся карта · Tab или стрелки: смена державы · 1–8: держава · G: сетка · Esc: назад
shell.observer.view-single = Одна держава
shell.observer.view-grid = Сетка
shell.observer.view-single-tip = Смотреть одну державу на весь экран (G)
shell.observer.view-grid-tip = Назад к сетке всех держав (G или Esc)
shell.observer.camera-follow = Следить
shell.observer.camera-free = Свободная камера
shell.observer.camera-follow-tip = Камера сама берёт города и обзор державы (F)
shell.observer.camera-free-tip = Тяните карту, крутите колесо: камера остаётся там, где вы её оставили
shell.observer.watch-all = Все державы
shell.observer.watch-one = Одна держава
shell.observer.watch-tip = Открыть наблюдателя сеткой всех держав или одной державой
shell.observer.events-title = Недавние события
shell.observer.no-events = Пока ничего
shell.observer.event-line = Ход { $round }: { $text }
shell.observer.ev-founded = Основан город { $city }
shell.observer.ev-captured = Взят город { $city }: у державы { $nation }
shell.observer.ev-captured-plain = Взят город { $city }
shell.observer.ev-lost = Потерян город { $city }: взяла держава { $nation }
shell.observer.ev-lost-plain = Потерян город { $city }
shell.observer.ev-war = Война с державой { $nation }
shell.observer.ev-peace = Мир с державой { $nation }
shell.observer.ev-learned = Открыто: { $tech }
shell.observer.ev-fell = Держава пала
shell.observer.finale-title = Партия окончена
shell.observer.winner = Победила держава { $name }
shell.observer.winners = Победили вместе: { $names }
shell.observer.nobody = Победителя нет
shell.observer.finale-line = Ход { $round } · { $reason }
shell.observer.cannot-play = Наблюдаемую партию нельзя продолжить самому
shell.observer.keep-playing = Играть дальше
shell.observer.to-menu = В главное меню
shell.observer.final-table = ИТОГОВЫЙ СЧЁТ
shell.observer.no-save = Наблюдаемую партию нельзя сохранить
shell.observer.reveal = Вся карта
shell.observer.reveal-tip = Открыть всю карту в каждом виде: без тумана, все города и отряды как есть сейчас (R)
shell.observer.reveal-tag = Вся карта открыта · R: вернуть туман
shell.observer.cities = Города
shell.observer.cities-tip = Карточки городов державы: что в каждом, без приказов (C)
shell.observer.city-place = Город { $place } из { $count }
shell.observer.card-hint = Стрелки влево и вправо или 1–9: другой город · Esc: закрыть
shell.observer.card-close = Закрыть
shell.observer.card-close-tip = Закрыть карточку (Esc)
shell.observer.card-prev-tip = Предыдущий город (стрелка влево)
shell.observer.card-next-tip = Следующий город (стрелка вправо)
shell.observer.city-click = Щёлкните, чтобы открыть карточку города
shell.observer.units-title = Отряды
shell.observer.unit-name = { $kind } ({ $nation })
shell.observer.unit-health = здоровье { $current } из { $max }
shell.observer.hp-value = { $current } из { $max }
shell.observer.unit-sight = обзор { $sight }
shell.observer.sight-unknown = обзор неизвестен
shell.observer.unit-fortified = укреплён
shell.observer.sight-label = Обзор
shell.observer.health-label = Здоровье
shell.observer.nation-label = Держава
shell.observer.unit-hint = Esc: снять выбор
shell.observer.sight-note = Обводка показывает, что он видит
shell.observer.key-pause.keys = Пробел
shell.observer.key-pause.what = Пауза и пуск
shell.observer.key-speed.keys = [ и ]
shell.observer.key-speed.what = Медленнее и быстрее
shell.observer.key-step.keys = N
shell.observer.key-step.what = Сыграть один ход на паузе
shell.observer.key-grid.keys = G
shell.observer.key-grid.what = Сетка всех держав или одна держава
shell.observer.key-tab.keys = Tab, Shift+Tab
shell.observer.key-tab.what = Другая держава (одна держава на весь экран)
shell.observer.key-arrows.keys = Стрелки влево и вправо
shell.observer.key-arrows.what = Другая держава; при открытой карточке города — другой город
shell.observer.key-numbers.keys = 1–8
shell.observer.key-numbers.what = Держава по номеру; при открытой карточке города — город по номеру
shell.observer.key-cities.keys = C
shell.observer.key-cities.what = Карточки городов державы
shell.observer.key-reveal.keys = R
shell.observer.key-reveal.what = Вся карта открыта или туман на месте
shell.observer.key-follow.keys = F
shell.observer.key-follow.what = Камера следит за землёй державы
shell.observer.key-hover.keys = Наведение на клетку
shell.observer.key-hover.what = Что на ней: местность, доходы, улучшение, ресурс, владелец, отряды
shell.observer.key-click-city.keys = Щелчок по городу
shell.observer.key-click-city.what = Его карточка: что в нём, без приказов
shell.observer.key-click-unit.keys = Щелчок по отряду
shell.observer.key-click-unit.what = Выбрать отряд: обводка его обзора и сведения о нём
shell.observer.key-escape.keys = Esc
shell.observer.key-escape.what = Закрыть карточку, снять выбор, вернуться к сетке

## Lands & Laws
shell.end-turn-tip@lal = Завершить ход: ходят другие державы, затем считается экономика хода
shell.empire@lal = Империя

## shell.errors
shell.errors.cannot-create = Не удалось создать { $path }: { $error }
shell.errors.cannot-read = Не удалось прочитать { $path }: { $error }
shell.errors.cannot-remove = Не удалось удалить { $path }: { $error }
shell.errors.cannot-read-save = Не удалось прочитать сохранение: { $error }
shell.errors.cannot-save = Не удалось сохранить: { $error }
shell.errors.storage-full = хранилище браузера заполнено; удалите старые сохранения или освободите место для этого сайта
shell.errors.cannot-rotate = Не удалось обновить автосохранения: { $error }
shell.errors.cannot-write-journal = Не удалось записать журнал приказов: { $error }
shell.errors.cannot-save-settings = Не удалось сохранить настройки: { $error }
shell.errors.hall-open = Не удалось открыть Зал славы: { $error }
shell.errors.hall-write = Не удалось записать в Зал славы: { $error }

## shell.showcase: the research sample of the showcase
shell.showcase.research = ИССЛЕДОВАНИЕ
shell.showcase.research-name = Горное дело
shell.showcase.research-turns = 3 хода
