"""Hunyuan3D-2.1 shape: python gen_hy.py <out_dir> <octree> <steps> <img:seed> ..."""
import sys, os, json, time, torch
sys.path.insert(0, os.path.expanduser('~/Hunyuan3D-2.1/hy3dshape'))
sys.path.insert(0, os.path.expanduser('~/Hunyuan3D-2.1'))
from PIL import Image
from hy3dshape.pipelines import Hunyuan3DDiTFlowMatchingPipeline
from hy3dshape.rembg import BackgroundRemover
out, octree, steps = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
os.makedirs(out, exist_ok=True)
pipe = Hunyuan3DDiTFlowMatchingPipeline.from_pretrained('tencent/Hunyuan3D-2.1')
pipe.enable_flashvdm(mc_algo='mc')
rembg = BackgroundRemover()
for job in sys.argv[4:]:
    img_path, seed = job.rsplit(':', 1); seed = int(seed)
    stem = f"hy21_{os.path.splitext(os.path.basename(img_path))[0]}_s{seed}_o{octree}"
    if os.path.exists(f"{out}/{stem}.glb"): continue
    im = Image.open(img_path).convert('RGB')
    if max(im.size) > 1600: im.thumbnail((1600, 1600))
    im = rembg(im)
    im.save(f"{out}/{stem}_input.png")
    t = time.time()
    mesh = pipe(image=im, num_inference_steps=steps, guidance_scale=5.0, octree_resolution=octree,
                num_chunks=200000, generator=torch.manual_seed(seed), output_type='trimesh')[0]
    mesh.export(f"{out}/{stem}.glb")
    json.dump(dict(generator='Hunyuan3D-2.1 shape (tencent/Hunyuan3D-2.1, hunyuan3d-dit-v2-1)', input=os.path.basename(img_path),
                   seed=seed, octree_resolution=octree, steps=steps, guidance_scale=5.0, rembg='hy3dshape BackgroundRemover',
                   verts=len(mesh.vertices), faces=len(mesh.faces), seconds=round(time.time()-t,1)), open(f"{out}/{stem}.json", 'w'), indent=1)
    print('DONE', stem, len(mesh.vertices), time.time()-t, flush=True)
