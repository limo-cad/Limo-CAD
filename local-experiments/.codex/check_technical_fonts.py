from fontTools.ttLib import TTFont
cs=[0x2300,0x2334,0x2335,0x21a7,0x2316,0x24c2,0x232d,0x232f,0x25b1,0x2312,0x2313,0x2322]
for file in ['seguisym.ttf','segoeui.ttf','cambria.ttc']:
    f=TTFont('C:/Windows/Fonts/'+file, fontNumber=0)
    print(file, {hex(c): f.getBestCmap().get(c) for c in cs})
