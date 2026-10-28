set -ex
export PATH=$HOME/.local/bin:$PATH CUDA_HOME=/usr/local/cuda TORCH_CUDA_ARCH_LIST=8.9 MAX_JOBS=2
cd ~/Hunyuan3D-2.1
uv venv -p 3.10 ~/venv-hy
source ~/venv-hy/bin/activate
uv pip install torch==2.5.1 torchvision==0.20.1 torchaudio==2.5.1 --index-url https://download.pytorch.org/whl/cu124
uv pip install pip setuptools wheel
grep -v -e "^bpy" -e deepspeed -e '^--extra' requirements.txt > /tmp/hyreq.txt
uv pip install -r /tmp/hyreq.txt
cd hy3dpaint/custom_rasterizer && pip install -e . --no-build-isolation && cd ../..
cd hy3dpaint/DifferentiableRenderer && bash compile_mesh_painter.sh && cd ../..
mkdir -p hy3dpaint/ckpt && wget -q https://github.com/xinntao/Real-ESRGAN/releases/download/v0.1.0/RealESRGAN_x4plus.pth -P hy3dpaint/ckpt
echo HY_DONE
