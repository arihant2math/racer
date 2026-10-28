# Image-to-3D on an AWS GPU

Scripts used to generate the car base meshes (`data/cars/<id>/refs/base_mesh/`).
Ran on a g6e.xlarge (L40S 48 GB, 4 vCPU, 32 GB RAM) with the Deep Learning
OSS Nvidia Driver AMI GPU PyTorch 2.7 (Ubuntu 22.04), 250 GB gp3, us-east-1c.
Add swap (64 GB) on the 32 GB box: Hunyuan's volume decode at octree 1024 OOMs.

```sh
git clone --recursive https://github.com/microsoft/TRELLIS.2 ~/TRELLIS.2
git clone https://github.com/Tencent-Hunyuan/Hunyuan3D-2.1 ~/Hunyuan3D-2.1
bash download_weights.sh     # ~33 GB of HF weights
bash setup_trellis.sh        # venv ~/venv-trellis (torch 2.6 cu124, flash-attn, nvdiffrast, cumesh, flexgemm, o-voxel)
bash setup_hunyuan.sh        # venv ~/venv-hy (torch 2.5.1 cu124, custom rasterizer)
```

TRELLIS.2 (used for all three cars): `cd ~/TRELLIS.2 && ~/venv-trellis/bin/python gen_trellis.py <out_dir> 1536_cascade <photo>:<seed> ...`
writes `tr2_<photo>_s<seed>_1536_cascade.glb` (1M faces after o-voxel remesh,
4096 PBR texture) plus a provenance JSON per candidate; about 3-5 min each.

Hunyuan3D-2.1 (`gen_hy.py`, FlashVDM decoder, shape only) works but was not
used for the final bases (TRELLIS.2 scored higher on the blueprint IoU).

Then locally:

```sh
python tools/car_refs/score_candidates.py data/cars/<id> candidates.json <dir>/tr2_*.glb
python tools/car_refs/normalize_base.py data/cars/<id> candidates.json <dir>
tools/build_cars.sh <id>
```
