"""Cantiva 的 TTS worker：把各模型的 Python API 包成同一個本機 HTTP 協定。

只用 Python 標準函式庫，才不會和各模型自己的套件版本衝突。
每個引擎在 engines/<id>/adapter.py 提供一個 `Adapter` 類別，介面見 base.py。
"""

__version__ = "0.1.0"
