"""Development-only numerical reference: pip install onnxruntime numpy. Official hash-pinned weights only."""
import hashlib,json,os,pathlib
import numpy as np
import onnxruntime as ort
ROOT=pathlib.Path(__file__).resolve().parents[1]
MODELS=pathlib.Path(os.environ["LIGHTCRAFT_TAG_MODELS"])
references={}
for name,size,digest in [
 ("yolox-nano",416,"c789161ed43c8269fcd4e67c67eeeb4e80c622da2eb296a20bc6007bd18a0b7d"),
 ("yolox-s",640,"c5c2d13e59ae883e6af3b45daea64af4833a4951c92d116ec270d9ddbe998063"),
]:
 path=MODELS/name/"model.onnx"
 assert hashlib.sha256(path.read_bytes()).hexdigest()==digest
 data=(np.arange(3*size*size,dtype=np.uint32)%251).astype(np.float32).reshape(1,3,size,size)
 session=ort.InferenceSession(str(path),providers=["CPUExecutionProvider"])
 output=session.run(None,{"images":data})[0].flatten()
 references[name]=output[::997].tolist()
(ROOT/"src/yolo-reference.json").write_text(json.dumps(references,separators=(",",":")),encoding="utf-8")
print("Saved independent ONNX Runtime samples for both official models")
