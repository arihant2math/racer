import sys
from PIL import Image
pre=sys.argv[1]; names=sys.argv[2].split(','); out=sys.argv[3]; cols=int(sys.argv[4]) if len(sys.argv)>4 else 2
ims=[Image.open(f'{pre}_{n}.png').convert('RGB') for n in names]
w,h=ims[0].size; rows=(len(ims)+cols-1)//cols
s=Image.new('RGB',(w*cols,h*rows),'black')
for i,im in enumerate(ims): s.paste(im,((i%cols)*w,(i//cols)*h))
s.thumbnail((2400,2400)); s.save(out)
