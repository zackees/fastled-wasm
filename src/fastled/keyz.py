# This was an experiment for https://localhost
# Important for audio sampling from sites like Youtube or an app like Audacity.
# However, pkg_resources does not work in the exe version of this app.
# For this to work pyinstaller needs special handling (probably a spec-file)
# in order to properly package up external resources.


from dataclasses import dataclass

_ENABLE_SSL_CONFIG = False


@dataclass
class SslConfig:
    cert: str
    key: str


def get_ssl_config() -> SslConfig | None:
    """Get the keys for the server"""
    if not _ENABLE_SSL_CONFIG:
        return None
    return SslConfig(
        cert=_CERT,
        key=_PRIVATE_KEY,
    )


_PRIVATE_KEY = """-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDlxbWcpUXPpjqs
DJPgFF1FsXPZqq0JPJqssHh4ZLfN0h4yJmj+kRcHS+pkgXnG46g6bUcL/AK5Ba08
vwnUUGkPH0v4ShKiAGYwvOcbWaqTmvvJuIoaDBXh2jSCeOTagNoaHLYEugARkkEu
0/FEW5P/79wU5vJ5G+SyZ8rBCVdxlU57pL1hKWBU7K+BLWsCiZ308NMpzHF5APZ6
YxVjhFosJPr4TjN6yXr+whrsAjSTHamD5690MbXWyyPG0jwPQyjBot/cNtt8GrsN
gcjA1E+8VKFvxO8RvZanMZLb0CGEpt7u3oaJ/jprHEsw+/UhnG6Qhksm8C/DN9kP
hselewffAgMBAAECggEARjQ6YTo+Mkvf8WGGbRjLxteJRiBX7lKOD+V7aY2ce06P
21LREbbTCm+vljXZN2OnqvJomsjNLCsH21+jaTOIZg5x79LyDn2Au7N8CWdELwVT
mTbBO2Ql63P4R0UY54onGYNcOeV6z+OX9u7a8L/qYHCxFdHalBZpsfj0gjaQeStJ
JSnvGjo6tKkwC/nUmX01qEVQgUO1+39WYqCaIWjijZNXt6XiKclEuu1AkL0u6Mpt
CzvzEDrEA66D0Lvl3Tek9B4O16Oie5anNnNMHigwU9yVc6dI8vDCRSEiz7laPTFK
xzOCQmqPGClKXkX3U+OvZp/Ss9U26Wpu0AbRKTvzAQKBgQDsMR9NjMpOmUaWkAwl
1wlUxsZ9YkRuTy7R3RfIdYWj6Lcoc4/iN0qILFM7xidkHhYTFqnsnP1SQkV6lEHV
OalYxZu9F2l1rHPc8G5YWh/KOg1rAEI47MVT4iwhA2fw6JLti/rm25AeSTMjSTqu
ht3146036opcIF3v86oGUrSXDwKBgQD5CsNcwLeUDGXozjq62T8/mTYwd2Tw3aiY
KaGp+exAW321vYm5SKsMitBMGU2tGFlv5eptSI48h7SCpgnszaexw7yj30KuvqjG
bBqq/MsKuXHyn2sG0A7MJ6zfk+4l46B45blDJZ+x7xL0dyS4UCU3zUeesgSGo4zK
ZOspPIQCMQKBgQCk35VuWP1P6IbxyxPvxi/pUeh01gfWyMdyD9fuQrtLM8PHJQQn
cVlBvU9MxoHwzV+za3qqhNwAc+p0KtHZuiqQoUCZuqIPVpZ6gAtG+YJ/dA6xxrhz
bDRC3frYALyp2m/WCoTWaiYsPgTIePHRqqt+XbQo+DwlGyL3wSvKxijx2QKBgCb0
OwioEE70/X/DukX9szn0chh0pHJUiYl7gZD/yadraCdkRUWZC0BD+j7c+lxn4Z1y
HhAH+E+Zfm+tHwJOTLuufTQ4uMpygh2/TRCPyAaeaSdlLi17n8TpM84o6mg8yZ3/
eNH68Za4aYOZm0HFL30h++DjwXd534zM6keh8pgRAoGBAKUrsjDGjuSo8l1fi4Cq
INu/rQop2h/db02zyJP5q7NKhE1nqogeLwwn+2M/LtHQ1nIzZR+rvrLNgt6oWY31
sPsv8JUfVT8GmdhU9KKmizK6eUu3rWdj2+rJARmuEaPmHcD5O6oJaGU0qadqQP34
H+enwWmpyZXAIbEu/q63DFhV
-----END PRIVATE KEY-----"""

_CERT = """-----BEGIN _CERTIFICATE-----
MIIEfTCCAuWgAwIBAgIRAPb7jkLrCuqToG+s3AQYeuUwDQYJKoZIhvcNAQELBQAw
gakxHjAcBgNVBAoTFW1rY2VydCBkZXZlbG9wbWVudCBDQTE/MD0GA1UECww2REVT
S1RPUC1JMzcxOERPXFphY2ggVm9yaGllc0BERVNLVE9QLUkzNzE4RE8gKG5pdGVy
aXMpMUYwRAYDVQQDDD1ta2NlcnQgREVTS1RPUC1JMzcxOERPXFphY2ggVm9yaGll
c0BERVNLVE9QLUkzNzE4RE8gKG5pdGVyaXMpMB4XDTI1MDQyODAwMzk1MFoXDTI3
MDcyODAwMzk1MFowajEnMCUGA1UEChMebWtjZXJ0IGRldmVsb3BtZW50IGNlcnRp
ZmljYXRlMT8wPQYDVQQLDDZERVNLVE9QLUkzNzE4RE9cWmFjaCBWb3JoaWVzQERF
U0tUT1AtSTM3MThETyAobml0ZXJpcykwggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAw
ggEKAoIBAQDlxbWcpUXPpjqsDJPgFF1FsXPZqq0JPJqssHh4ZLfN0h4yJmj+kRcH
S+pkgXnG46g6bUcL/AK5Ba08vwnUUGkPH0v4ShKiAGYwvOcbWaqTmvvJuIoaDBXh
2jSCeOTagNoaHLYEugARkkEu0/FEW5P/79wU5vJ5G+SyZ8rBCVdxlU57pL1hKWBU
7K+BLWsCiZ308NMpzHF5APZ6YxVjhFosJPr4TjN6yXr+whrsAjSTHamD5690MbXW
yyPG0jwPQyjBot/cNtt8GrsNgcjA1E+8VKFvxO8RvZanMZLb0CGEpt7u3oaJ/jpr
HEsw+/UhnG6Qhksm8C/DN9kPhselewffAgMBAAGjXjBcMA4GA1UdDwEB/wQEAwIF
oDATBgNVHSUEDDAKBggrBgEFBQcDATAfBgNVHSMEGDAWgBSPBydvhr9wI+FsoW/H
WK3DbS8IUDAUBgNVHREEDTALgglsb2NhbGhvc3QwDQYJKoZIhvcNAQELBQADggGB
AJVrF1yczZaxt+A2AhdeFbJQUR6NzGBTc20YeWF1YzLV5sV3QVumwZLP2M9ggRgd
xWV0xfwUHobFQk6RIPTADcLKctiurql0cgF4DPnpWVvto9RM00U3AkQcMj3xtKBV
wUqo83TcbqgL+euudFZ09gGTs9u9AENaZPcMh+rW8DDO92t+EwMI/IfopxVOJGUB
RSM3yTwV93BMYBuddt8mclzLzPK/1WONfsHU2xEascaHR1tYMOmJN9Vq4o0fzWxo
a2vI6K0aJqZV/ztdXq3akwLc6/e9hyptHWa0i/022xVCyNWIlnuEhT7ENMPxh6rX
ZCQCZVnhcSWAyFjggLJql3aSID5fPF8rmN7wWsB/I5pl9qwMR1/THMPrm5aWn1Xj
xW6PxkSGm73kd57DH7tqm5HTd8eYCbnsFofI9rC7xI6HCfwchKp+YHvIEu/LJ56E
FLnCZW/orYkHCzWntzxv1bddrw1BwaNR8Q+mu3imRP8fuyXb2UkFkINVVyOOWHuW
Kw==
-----END _CERTIFICATE-----"""
