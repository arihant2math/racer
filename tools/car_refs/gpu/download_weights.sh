export PATH=$HOME/.local/bin:$PATH HF_HUB_ENABLE_HF_TRANSFER=0
H="uvx --from huggingface_hub hf download"
$H microsoft/TRELLIS.2-4B
$H microsoft/TRELLIS-image-large --include "ckpts/ss_dec_conv3d_16l8_fp16*" pipeline.json
$H camenduru/dinov3-vitl16-pretrain-lvd1689m
$H ZhengPeng7/BiRefNet
$H tencent/Hunyuan3D-2.1 --include "hunyuan3d-dit-v2-1/*" "hunyuan3d-vae-v2-1/*" "hunyuan3d-paintpbr-v2-1/*" "*.json"
$H tencent/Hunyuan3D-2mv --include "hunyuan3d-dit-v2-mv/*" "hunyuan3d-vae-v2-0/*" "*.json" || true
$H facebook/dinov2-giant || true
echo DL_DONE
