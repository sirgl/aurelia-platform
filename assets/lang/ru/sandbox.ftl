# Translation of sandbox.ftl into ru. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

sandbox.lib.title = Песочница
sandbox.lib.found-city = Основать город
sandbox.lib.match-summary = { $faction } · мир { $width } × { $height }
sandbox.lib.unit-line = { $hp } ОЗ · { $moves ->
    [one] { $moves } очко движения
    [few] { $moves } очка движения
    [many] { $moves } очков движения
   *[other] { $moves } очка движения
}
sandbox.lib.unit-line-civilian = { $hp } ОЗ · { $moves ->
    [one] { $moves } очко движения
    [few] { $moves } очка движения
    [many] { $moves } очков движения
   *[other] { $moves } очка движения
} · мирный
sandbox.lib.hall-of-fame = Песочница · { $faction } · { $outcome } на ходу { $round }
sandbox.lib.outcome-playing = Идёт игра
sandbox.lib.outcome-victory = Победа
sandbox.lib.outcome-defeat = Поражение
sandbox.lib.key-left-click = Левый клик
sandbox.lib.key-left-click-help = Выбрать отряд или открыть город
sandbox.lib.key-right-click = Правый клик
sandbox.lib.key-right-click-help = Отправить туда отряд или атаковать
sandbox.screens.city-title = Город
sandbox.screens.new-game-title = Новая игра в песочнице
sandbox.screens.sandbox-heading = Песочница
sandbox.screens.about = Небольшая 4X-игра на ядре платформы: четыре державы основывают города, развивают их на еде и производстве и борются за каждую столицу. На 100-м ходу побеждает самая большая держава.
sandbox.screens.start-game = Начать игру
sandbox.screens.choose-city = Выберите один из своих городов на карте.
sandbox.screens.city-line = Население { $population } · еды: { $food } · производства: { $production }
sandbox.screens.producing = ПРОИЗВОДИТСЯ
sandbox.screens.producing-item = { $item } — накоплено { $stored }
sandbox.screens.producing-nothing = Ничего: выберите, что строить
sandbox.screens.build-next = СТРОИТЬ ДАЛЬШЕ
sandbox.screens.turns = { $n ->
    [one] { $n } ход
    [few] { $n } хода
    [many] { $n } ходов
   *[other] { $n } хода
}
sandbox.screens.never = никогда
sandbox.screens.offer = { $name }   { $cost } · { $turns }
sandbox.screens.esc-hint = Esc — вернуться к карте.
sandbox.session.not-a-save = Это не сохранение песочницы ({ $detail })
sandbox.session.save-failed = Сохранение не загружается ({ $detail })
sandbox.session.top-food = { $amount } еды
sandbox.session.top-production = { $amount } производства
sandbox.session.top-cities = { $n ->
    [one] { $n } город
    [few] { $n } города
    [many] { $n } городов
   *[other] { $n } города
}
