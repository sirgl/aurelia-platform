#!/usr/bin/env python3
"""Writes assets/lang/<code>/glossary.ftl from the table below (the glossary is the
translators' agreement on core terms: edit the table, rerun, review the diff)."""
import os, re, sys
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
# key | fr | de | es | zh-Hans | ja
T = """
city|ville|Stadt|ciudad|城市|都市
capital|capitale|Hauptstadt|capital|首都|首都
nation|nation|Nation|nación|国家|国家
faction|faction|Fraktion|facción|势力|勢力
citizen|citoyen|Bürger|ciudadano|市民|市民
population|population|Bevölkerung|población|人口|人口
tile|case|Feld|casilla|地块|タイル
territory|territoire|Gebiet|territorio|领土|領土
border|frontière|Grenze|frontera|边境|国境
city-state|cité-État|Stadtstaat|ciudad-estado|城邦|都市国家
barbarians|barbares|Barbaren|bárbaros|蛮族|蛮族
tribe|tribu|Stamm|tribu|部落|部族
ruins|ruines|Ruinen|ruinas|遗迹|遺跡
turn|tour|Zug|turno|回合|ターン
era|ère|Zeitalter|era|时代|時代
technology|technologie|Technologie|tecnología|科技|テクノロジー
research|recherche|Forschung|investigación|研究|研究
policy|politique|Sozialpolitik|política|政策|政策
law|loi|Gesetz|ley|法律|法
wonder|merveille|Wunder|maravilla|奇观|驚異
victory|victoire|Sieg|victoria|胜利|勝利
score|score|Punktzahl|puntuación|得分|スコア
promotion|promotion|Beförderung|ascenso|晋升|昇進
great-person|grand personnage|Große Persönlichkeit|gran personaje|伟人|偉人
food|nourriture|Nahrung|comida|食物|食料
production|production|Produktion|producción|产能|生産力
gold|or|Gold|oro|金币|ゴールド
science|science|Wissenschaft|ciencia|科研|科学力
culture|culture|Kultur|cultura|文化|文化
faith|foi|Glaube|fe|信仰|信仰
happiness|bonheur|Zufriedenheit|felicidad|幸福|幸福度
upkeep|entretien|Unterhalt|mantenimiento|维护费|維持費
income|revenu|Einkommen|ingresos|收入|収入
treasury|trésor|Staatskasse|tesoro|国库|国庫
trade|commerce|Handel|comercio|贸易|貿易
trade-route|route commerciale|Handelsroute|ruta comercial|贸易路线|交易路
unit|unité|Einheit|unidad|单位|ユニット
civilian|unité civile|Zivileinheit|unidad civil|平民单位|民間ユニット
military|unité militaire|Militäreinheit|unidad militar|军事单位|軍事ユニット
settler|colon|Siedler|colono|开拓者|開拓者
worker|ouvrier|Arbeiter|trabajador|工人|労働者
scout|éclaireur|Späher|explorador|侦察兵|斥候
warrior|guerrier|Krieger|guerrero|战士|戦士
archer|archer|Bogenschütze|arquero|弓箭手|弓兵
spearman|lancier|Speerkämpfer|lancero|枪兵|槍兵
swordsman|épéiste|Schwertkämpfer|espadachín|剑士|剣士
horseman|cavalier|Reiter|jinete|骑兵|騎兵
catapult|catapulte|Katapult|catapulta|投石机|カタパルト
ship|navire|Schiff|barco|舰船|船
strength|force|Stärke|fuerza|战斗力|戦闘力
ranged-strength|force à distance|Fernkampfstärke|fuerza a distancia|远程战斗力|遠隔戦闘力
moves|mouvement|Bewegung|movimiento|移动力|移動力
sight|vision|Sicht|visión|视野|視界
health|santé|Gesundheit|salud|生命值|体力
fortify|se fortifier|befestigen|fortificar|设防|要塞化
pillage|piller|plündern|saquear|掠夺|略奪
garrison|garnison|Garnison|guarnición|驻军|守備隊
siege|siège|Belagerung|asedio|攻城|攻城
building|bâtiment|Gebäude|edificio|建筑|建造物
improvement|aménagement|Verbesserung|mejora|改良设施|改良
road|route|Straße|camino|道路|道路
resource|ressource|Ressource|recurso|资源|資源
luxury|ressource de luxe|Luxusgut|recurso de lujo|奢侈品资源|贅沢資源
strategic|ressource stratégique|Strategische Ressource|recurso estratégico|战略资源|戦略資源
terrain|terrain|Gelände|terreno|地形|地形
feature|élément|Geländemerkmal|rasgo|地貌|地形特性
river|rivière|Fluss|río|河流|川
coast|côte|Küste|costa|海岸|海岸
hills|collines|Hügel|colinas|丘陵|丘陵
mountain|montagne|Berg|montaña|山脉|山岳
forest|forêt|Wald|bosque|森林|森林
jungle|jungle|Dschungel|jungla|丛林|ジャングル
desert|désert|Wüste|desierto|沙漠|砂漠
tundra|toundra|Tundra|tundra|苔原|ツンドラ
plains|plaines|Ebene|llanura|平原|平地
grassland|prairie|Grasland|pradera|草原|草原
ocean|océan|Ozean|océano|海洋|海洋
fog|brouillard de guerre|Kriegsnebel|niebla de guerra|战争迷雾|戦場の霧
diplomacy|diplomatie|Diplomatie|diplomacia|外交|外交
war|guerre|Krieg|guerra|战争|戦争
peace|paix|Frieden|paz|和平|平和
treaty|traité|Vertrag|tratado|条约|条約
deal|accord|Abkommen|acuerdo|交易|取引
ally|allié|Verbündeter|aliado|盟友|同盟国
enemy|ennemi|Feind|enemigo|敌人|敵
""".strip().split('\n')
# Russian is NOT generated here: assets/lang/ru/glossary.ftl is maintained by hand (the repository keeps
# Russian text only under assets/lang/ru, see docs/localization.md).
LANGS = ['fr', 'de', 'es', 'zh-Hans', 'ja']
en_text = open(os.path.join(ROOT, 'assets/lang/en/glossary.ftl'), encoding='utf-8').read()
en_keys = re.findall(r'^glossary\.([a-z-]+) = ', en_text, flags=re.M)
rows = {}
for line in T:
    parts = line.split('|')
    assert len(parts) == 6, line
    rows[parts[0]] = parts[1:]
missing = [k for k in en_keys if k not in rows]
extra = [k for k in rows if k not in en_keys]
assert not missing and not extra, (missing, extra)
for i, lang in enumerate(LANGS):
    out = []
    for ln in en_text.split('\n'):
        m = re.match(r'^glossary\.([a-z-]+) = ', ln)
        if m:
            out.append(f'glossary.{m.group(1)} = {rows[m.group(1)][i]}')
        elif ln.startswith('# ') or ln == '#':
            continue  # the English file explains; translations carry sections only
        else:
            out.append(ln)
    text = '# Glossary: the core terms (see the English file). Edit tools/i18n/gen_glossary.py or this file.\n' + '\n'.join(out).strip('\n') + '\n'
    d = os.path.join(ROOT, 'assets/lang', lang)
    os.makedirs(d, exist_ok=True)
    open(os.path.join(d, 'glossary.ftl'), 'w', encoding='utf-8').write(text)
    print(lang, len(rows))
