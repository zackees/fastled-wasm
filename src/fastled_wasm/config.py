import json

import keyring

KEYRING_SERVICE = "fastled_wasm"
KEYRING_USERNAME = "settings"


class Config:
    def __init__(self, last_volume_path: str = "") -> None:
        json_data = keyring.get_password(KEYRING_SERVICE, KEYRING_USERNAME)
        if json_data:
            self.__dict__.update(json.loads(json_data))
        self.last_volume_path = last_volume_path

    @classmethod
    def from_dict(cls, data: dict) -> "Config":
        return cls(**data)

    def to_dict(self) -> dict:
        return {"last_volume_path": self.last_volume_path}

    def save(self) -> None:
        """Save config to keyring"""
        keyring.set_password(
            KEYRING_SERVICE, KEYRING_USERNAME, json.dumps(self.to_dict())
        )

    def load(self) -> None:
        """Load config from keyring"""
        try:
            settings_json = keyring.get_password(KEYRING_SERVICE, KEYRING_USERNAME)
            if settings_json:
                self.__dict__.update(json.loads(settings_json))
        except Exception as e:
            print(f"Error loading settings: {e}")
