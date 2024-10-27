import json

import keyring

KEYRING_SERVICE = "fastled_wasm"
KEYRING_USERNAME = "settings"


class Config:
    def __init__(self) -> None:
        try:
            json_data = keyring.get_password(KEYRING_SERVICE, KEYRING_USERNAME)
            if json_data:
                data = json.loads(json_data)
                self.__dict__.update(data)
            self.last_volume_path = data.get("last_volume_path", "NO_LAST_VOLUME_PATH")
        except Exception as e:
            print(
                f"Error loading settings, keyring might not be available: {e}, the compiler will not remember the last volume path and will re-deploy the docker container always."
            )
            self.last_volume_path = "COULD-NOT-LOAD-KEYRING-MIGHT-BE-UNAVAILABLE"

    @classmethod
    def from_dict(cls, data: dict) -> "Config":
        return cls(**data)

    def to_dict(self) -> dict:
        return {"last_volume_path": self.last_volume_path}

    def save(self) -> None:
        """Save config to keyring"""
        try:
            keyring.set_password(
                KEYRING_SERVICE, KEYRING_USERNAME, json.dumps(self.to_dict())
            )
        except Exception:
            pass

    def load(self) -> None:
        """Load config from keyring"""
        try:
            settings_json = keyring.get_password(KEYRING_SERVICE, KEYRING_USERNAME)
            if settings_json:
                self.__dict__.update(json.loads(settings_json))
        except Exception as e:
            print(f"Error loading settings: {e}")
