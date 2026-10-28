set -ex
export PATH=$HOME/.local/bin:$PATH CUDA_HOME=/usr/local/cuda TORCH_CUDA_ARCH_LIST=8.9 MAX_JOBS=4
cd ~/TRELLIS.2
uv venv -p 3.10 ~/venv-trellis
source ~/venv-trellis/bin/activate
uv pip install torch==2.6.0 torchvision==0.21.0 --index-url https://download.pytorch.org/whl/cu124
uv pip install pip setuptools wheel ninja packaging psutil
uv pip install imageio imageio-ffmpeg tqdm easydict opencv-python-headless trimesh transformers gradio==6.0.1 tensorboard pandas lpips zstandard kornia timm pillow huggingface_hub
uv pip install git+https://github.com/EasternJournalist/utils3d.git@9a4eb15e4021b67b12c460c7057d642626897ec8
pip install flash-attn==2.7.3 --no-build-isolation
mkdir -p /tmp/extensions
git clone -b v0.4.0 https://github.com/NVlabs/nvdiffrast.git /tmp/extensions/nvdiffrast || true
pip install /tmp/extensions/nvdiffrast --no-build-isolation
git clone -b renderutils https://github.com/JeffreyXiang/nvdiffrec.git /tmp/extensions/nvdiffrec || true
pip install /tmp/extensions/nvdiffrec --no-build-isolation
git clone https://github.com/JeffreyXiang/CuMesh.git /tmp/extensions/CuMesh --recursive || true
pip install /tmp/extensions/CuMesh --no-build-isolation
git clone https://github.com/JeffreyXiang/FlexGEMM.git /tmp/extensions/FlexGEMM --recursive || true
pip install /tmp/extensions/FlexGEMM --no-build-isolation
rm -rf /tmp/extensions/o-voxel; cp -r o-voxel /tmp/extensions/o-voxel
pip install /tmp/extensions/o-voxel --no-build-isolation
echo TRELLIS_DONE

# --- Local tweaks used for the committed base meshes -------------------------
# transformers 5.x renamed DINOv3 internals; pin 4.56.
uv pip install "transformers==4.56.2"
# No HF token: facebook/dinov3 and briaai/RMBG-2.0 are gated. Use an ungated
# DINOv3 mirror (safetensors only) and the original BiRefNet (RMBG-2.0's base).
S=$(ls -d ~/.cache/huggingface/hub/models--microsoft--TRELLIS.2-4B/snapshots/*)
mkdir -p ~/trellis2-local && ln -sfn "$S/ckpts" ~/trellis2-local/ckpts
python - "$S/pipeline.json" <<'P'
import json, os, sys
p = json.load(open(sys.argv[1]))
p["args"]["image_cond_model"]["args"]["model_name"] = "camenduru/dinov3-vitl16-pretrain-lvd1689m"
p["args"]["rembg_model"]["args"]["model_name"] = "ZhengPeng7/BiRefNet"
json.dump(p, open(os.path.expanduser("~/trellis2-local/pipeline.json"), "w"), indent=1)
P
