import json, sys
from fontTools.ttLib import TTFont
from fontTools import subset
from fontTools.merge import Merger
REG = dict(PLAY='play',PAUSE='pause',REPLAY='arrow_counterclockwise',NEXT='next',SETTINGS='settings',
  GROUP='people',PERSON_ADD='person_add',FULLSCREEN='full_screen_maximize',FULLSCREEN_EXIT='full_screen_minimize',
  CLOSE='dismiss',DELETE='delete',SHUTTLE='sport',SEARCH='search',HOURGLASS='hourglass',COFFEE='drink_coffee',
  LOGOUT='arrow_exit',LOGIN='arrow_enter_left',ADD='add',REMOVE='subtract',UNDO='arrow_undo',UP='arrow_up',
  NIGHT='weather_moon',CHECK='checkmark',HISTORY='history',CALENDAR='calendar_ltr',CLOCK='clock',
  CHEVRON_UP='chevron_up',CHEVRON_DOWN='chevron_down',INFO='info',WARNING='warning',CHECK_CIRCLE='checkmark_circle',
  TIMER='timer',PERSON='person',ARROW_SWAP='arrow_swap',QUESTION='question_circle',EDIT='edit',PERSON_STAR='person_star',DOWNLOAD='arrow_download')
FIL = dict(PLAY_F='play',PAUSE_F='pause',NEXT_F='next',SHUTTLE_F='sport',GROUP_F='people',CHECK_CIRCLE_F='checkmark_circle',
  INFO_F='info',WARNING_F='warning',HOURGLASS_F='hourglass',ARROW_UP_F='arrow_circle_up',TIMER_F='timer',QUESTION_F='question_circle',DOWNLOAD_F='arrow_download')
out = []
def sub(ttf, js, names, style, remap_base=None):
    m = json.load(open(js)); cps = []
    for const, n in names.items():
        cps.append(m[f'ic_fluent_{n}_24_{style}'])
    f = TTFont(ttf)
    opts = subset.Options(); opts.layout_features = []; opts.notdef_outline = True
    s = subset.Subsetter(opts); s.populate(unicodes=cps); s.subset(f)
    if remap_base is not None:
        cmap = f.getBestCmap(); new = {}
        for i,(const,n) in enumerate(names.items()):
            new[remap_base+i] = cmap[cps[i]]
            out.append((const, remap_base+i))
        f['cmap'].tables = [t for t in f['cmap'].tables if t.format == 12 or t.format==4]
        from fontTools.ttLib.tables._c_m_a_p import CmapSubtable
        t = CmapSubtable.newSubtable(12); t.platformID=3; t.platEncID=10; t.language=0; t.cmap=new
        f['cmap'].tables = [t]
    else:
        for i,(const,n) in enumerate(names.items()): out.append((const, cps[i]))
    return f
r = sub('FluentSystemIcons-Regular.ttf','FluentSystemIcons-Regular.json',REG,'regular'); r.save('r.ttf')
f = sub('FluentSystemIcons-Filled.ttf','FluentSystemIcons-Filled.json',FIL,'filled',0x100000); f.save('f.ttf')
merged = Merger().merge(['r.ttf','f.ttf']); merged.save('FluentIcons-DropIn.ttf')
for c,cp in out: print(f'    pub const {c}: &str = "\\u{{{cp:x}}}";')
