"""Dev/CI only. Fetch the model registry emitted by tagging-bench, verifying size and SHA-256."""
import hashlib,json,os,pathlib,subprocess,sys,urllib.request

def verified(path, model):
    if not path.is_file() or path.stat().st_size != model["bytes"]: return False
    digest=hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1048576),b""): digest.update(chunk)
    return digest.hexdigest()==model["sha256"]

def main():
    root=pathlib.Path(os.environ["LIGHTCRAFT_TAG_MODELS"])
    models=json.loads(subprocess.check_output([sys.argv[1],"--models"],text=True))
    for model in models:
        target=root/model["id"]/model["file"]
        if not verified(target,model):
            target.parent.mkdir(parents=True,exist_ok=True)
            part=target.with_suffix(target.suffix+".part")
            print("Downloading",model["id"],flush=True)
            urllib.request.urlretrieve(model["url"],part)
            if not verified(part,model): raise RuntimeError("Model size/checksum mismatch: "+model["id"])
            part.replace(target)
        print("Verified",model["id"],flush=True)

if __name__=="__main__": main()
