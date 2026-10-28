"""TRELLIS.2: python gen_trellis.py <out_dir> <pipeline_type> <img:seed> ..."""
import os, sys, json, time
os.environ.setdefault("PYTORCH_CUDA_ALLOC_CONF", "expandable_segments:True")
sys.path.insert(0, os.path.expanduser('~/TRELLIS.2'))
import torch
from PIL import Image
from trellis2.pipelines import Trellis2ImageTo3DPipeline
import o_voxel
out, ptype = sys.argv[1], sys.argv[2]
os.makedirs(out, exist_ok=True)
pipe = Trellis2ImageTo3DPipeline.from_pretrained(os.path.expanduser('~/trellis2-local'))
pipe.cuda()
for job in sys.argv[3:]:
    img_path, seed = job.rsplit(':', 1); seed = int(seed)
    stem = f"tr2_{os.path.splitext(os.path.basename(img_path))[0]}_s{seed}_{ptype}"
    if os.path.exists(f"{out}/{stem}.glb"): continue
    t = time.time()
    im = Image.open(img_path)
    pre = pipe.preprocess_image(im); pre.save(f"{out}/{stem}_input.png")
    mesh = pipe.run(pre, seed=seed, pipeline_type=ptype, preprocess_image=False, max_num_tokens=200000)[0]
    nv, nf = mesh.vertices.shape[0], mesh.faces.shape[0]
    mesh.simplify(16777216)
    glb = o_voxel.postprocess.to_glb(vertices=mesh.vertices, faces=mesh.faces, attr_volume=mesh.attrs, coords=mesh.coords,
        attr_layout=mesh.layout, voxel_size=mesh.voxel_size, aabb=[[-0.5]*3, [0.5]*3],
        decimation_target=1000000, texture_size=4096, remesh=True, remesh_band=1, remesh_project=0, verbose=False)
    glb.export(f"{out}/{stem}.glb")
    json.dump(dict(generator='TRELLIS.2-4B (microsoft/TRELLIS.2, image_to_3d)', input=os.path.basename(img_path), seed=seed,
        pipeline_type=ptype, max_num_tokens=200000, raw_verts=int(nv), raw_faces=int(nf), glb_decimation_target=1000000,
        texture_size=4096, image_cond='camenduru/dinov3-vitl16-pretrain-lvd1689m (mirror of facebook/dinov3-vitl16-pretrain-lvd1689m)',
        rembg='ZhengPeng7/BiRefNet', seconds=round(time.time()-t, 1)), open(f"{out}/{stem}.json", 'w'), indent=1)
    print('DONE', stem, nv, nf, time.time()-t, flush=True)
    torch.cuda.empty_cache()
