import os

os.system("docker compose down --remove-orphans --rmi all")
os.system("docker compose build")