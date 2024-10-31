import os
import subprocess
import argparse
from pathlib import Path

def clone_fastled_repo():
    """Clone the FastLED repository from GitHub."""
    repo_url = "https://github.com/fastled/fastled"
    repo_dir = "tmp/src/platforms/wasm/compiler"
    if os.path.exists(repo_dir):
        print("Repository directory exists, resetting and updating repository.")
        try:
            subprocess.run(["git", "-C", repo_dir, "reset", "--hard", "HEAD"], check=True)
            subprocess.run(["git", "-C", repo_dir, "pull"], check=True)
            print("Successfully reset and updated FastLED repository.")
        except subprocess.CalledProcessError as e:
            print(f"An error occurred while updating the repository: {e}")
    else:
        os.makedirs(repo_dir, exist_ok=True)
        try:
            subprocess.run(["git", "clone", repo_url, repo_dir], check=True)
            print("Successfully cloned FastLED repository.")
        except subprocess.CalledProcessError as e:
            print(f"An error occurred while cloning the repository: {e}")

def parse_arguments():
    """Parse command-line arguments."""
    parser = argparse.ArgumentParser(description="Build compilers script.")
    parser.add_argument("--docker-user", type=str, help="Docker username")
    parser.add_argument("--docker-password", type=str, help="Docker password")
    return parser.parse_args()

def main():
    """Main function to execute the script."""
    args = parse_arguments()
    print(f"Docker User: {args.docker_user}")
    clone_fastled_repo()
    try:
        # Log in to Docker Hub
        subprocess.run(
            ["docker", "login", "--username", args.docker_user, "--password-stdin"],
            input=args.docker_password.encode(),
            check=True
        )
        print("Docker login successful.")

        # Generate timestamp for image tagging
        timestamp = subprocess.check_output(["date", "+%s"]).strip().decode('utf-8')

        # Build the Docker image
        image_tag = f"niteris/fastled-wasm:{timestamp}"
        dockerfile_path = Path("tmp/src/platforms/wasm/compiler/Dockerfile")

        def run_and_print(command):
            result = subprocess.run(command, shell=True, capture_output=True, text=True)
            print(f"Command: {command}\nOutput:\n{result.stdout}\nError:\n{result.stderr}")

        run_and_print("pwd")
        run_and_print("ls tmp")
        run_and_print("ls tmp/src")
        run_and_print("ls tmp/src/platforms")
        run_and_print("ls tmp/src/platforms/wasm")
        run_and_print("ls tmp/src/platforms/wasm/compiler")
        run_and_print("ls tmp/src/platforms/wasm/compiler/Dockerfile")

        #if not os.path.exists(dockerfile_path):
       #     raise FileNotFoundError(f"Dockerfile not found at {dockerfile_path}")
        if not dockerfile_path.exists():
            raise FileNotFoundError(f"Dockerfile not found at {dockerfile_path}")
        dockerfile_path = Path("src/platforms/wasm/compiler/Dockerfile")


        

        
        cmd = ["docker", "build", ".", "--file", dockerfile_path, "--tag", image_tag]
        cmd_str = subprocess.list2cmdline(cmd)
        cwd = Path(os.getcwd())/"tmp"
        print(f"Building Docker image with command: {cmd_str} at cwd={cwd}")
        subprocess.run(
            cmd,
            cwd=str(cwd),
            shell=True,
            check=True
        )
        print(f"Docker image built with tag: {image_tag}")

        # Push the Docker image
        subprocess.run(
            ["docker", "push", image_tag],
            check=True
        )
        print(f"Docker image pushed: {image_tag}")

    except subprocess.CalledProcessError as e:
        print(f"An error occurred: {e}")

if __name__ == "__main__":
    main()
