import os
import subprocess
import argparse
import sys
from pathlib import Path

def clone_fastled_repo():
    """Clone the FastLED repository from GitHub."""
    repo_url = "https://github.com/fastled/fastled"
    repo_dir = "tmp"
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
        tmp = Path("tmp")
        os.chdir(str(tmp))
        print("Attempting to log in to Docker Hub...")
        subprocess.run(
            ["docker", "login", "--username", args.docker_user, "--password-stdin"],
            input=args.docker_password.encode(),
            check=True
        )
        print("Docker login successful.")

        print("Starting Docker image build process...")
        image_tag = f"niteris/fastled-wasm:latest"
        # Check common locations for the Dockerfile

        dockerfile_path = Path("src/platforms/wasm/compiler/Dockerfile")
        if not dockerfile_path.exists():
            raise FileNotFoundError(f"Dockerfile not found at {dockerfile_path}")

        #if not os.path.exists(dockerfile_path):
       #     raise FileNotFoundError(f"Dockerfile not found at {dockerfile_path}")
        if not dockerfile_path.exists():
            raise FileNotFoundError(f"Dockerfile not found at {dockerfile_path}")
        dockerfile_path = Path("src/platforms/wasm/compiler/Dockerfile")
        
        cmd = ["docker", "build", ".", "--file", dockerfile_path, "--tag", image_tag]
        cmd_str = subprocess.list2cmdline(cmd)
        print(f"Building Docker image with command: {cmd_str}")
        try:
            subprocess.run(
                cmd,
                shell=True,
                check=True
            )
            print(f"Docker image built with tag: {image_tag}")
        except subprocess.CalledProcessError as e:
            print(f"Failed to build Docker image: {e}")
            sys.exit(1)
        print(f"Docker image built with tag: {image_tag}")

        # Push the Docker image
        print("Attempting to push Docker image...")
        try:
            subprocess.run(
                ["docker", "push", image_tag],
                check=True
            )
            print(f"Docker image pushed: {image_tag}")
        except subprocess.CalledProcessError as e:
            print(f"Failed to push Docker image: {e}")
            sys.exit(1)

    except subprocess.CalledProcessError as e:
        print(f"An error occurred: {e}")
        sys.exit(1)

if __name__ == "__main__":
    main()
