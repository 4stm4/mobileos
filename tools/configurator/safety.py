"""
safety.py — input validation utilities for mobileos Configurator.

Все user-input, попадающий в shell-команды, ssh-аргументы или filesystem-пути,
ДОЛЖЕН проходить через эти проверки. Иначе — RCE.
"""
import re
from pathlib import Path

# Имена профилей/таргетов: только латиница, цифры, дефис, подчёркивание, точка.
# Никаких слешей, никаких .. — против path traversal.
_SAFE_NAME_RE = re.compile(r"^[A-Za-z0-9_][A-Za-z0-9_.\-]{0,63}$")

# Пути в filesystem (для shell-интерполяции): абсолютные, без shell-метасимволов.
# Разрешены только обычные символы: латиница, цифры, /, -, _, ., пробел НЕ разрешён.
_SAFE_PATH_RE = re.compile(r"^/[A-Za-z0-9_./\-]+$")

# Block-устройства: только канонические пути.
_SAFE_BLOCK_DEV_RE = re.compile(r"^/dev/(sd[a-z]\d?|mmcblk\d+(p\d+)?|nvme\d+n\d+(p\d+)?|disk\d+(s\d+)?)$")

# SSH-хосты: алиасы либо hostname.
_SAFE_HOST_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._\-]{0,253}$")


class UnsafeInputError(ValueError):
    """Raised when a user-supplied value fails validation."""


def safe_name(value: str, *, field: str = "name") -> str:
    """Profile/target/build-id name — alphanumeric + . _ -, no slashes, no ..."""
    if not isinstance(value, str) or not _SAFE_NAME_RE.match(value) or ".." in value:
        raise UnsafeInputError(f"unsafe {field}: {value!r}")
    return value


def safe_path(value: str, *, field: str = "path") -> str:
    """Absolute filesystem path with no shell metacharacters and no .. components."""
    if not isinstance(value, str) or not _SAFE_PATH_RE.match(value):
        raise UnsafeInputError(f"unsafe {field}: {value!r}")
    # Reject .. components even though the regex allows literal dots
    parts = value.split("/")
    if ".." in parts:
        raise UnsafeInputError(f"unsafe {field} (contains ..): {value!r}")
    return value


def safe_block_device(value: str) -> str:
    """Validate a block device path: /dev/sdX, /dev/mmcblkN, /dev/nvmeXnY, /dev/diskN."""
    if not isinstance(value, str) or not _SAFE_BLOCK_DEV_RE.match(value):
        raise UnsafeInputError(f"not a valid block device: {value!r}")
    return value


def safe_host(value: str) -> str:
    """SSH hostname or alias."""
    if not isinstance(value, str) or not _SAFE_HOST_RE.match(value):
        raise UnsafeInputError(f"unsafe host: {value!r}")
    return value


def safe_artifact(image: str, artifacts_dir: str) -> str:
    """Validate that `image` is a real file inside `artifacts_dir`, no traversal."""
    safe_path(image, field="image")
    safe_path(artifacts_dir, field="artifacts_dir")
    img = Path(image).resolve()
    art = Path(artifacts_dir).resolve()
    try:
        img.relative_to(art)
    except ValueError:
        raise UnsafeInputError(f"image {image!r} is not inside artifacts dir {artifacts_dir!r}")
    if not img.suffix.lower() in {".img", ".qcow2"}:
        raise UnsafeInputError(f"image {image!r} must end in .img or .qcow2")
    return str(img)


def safe_profile_path(profiles_dir: Path, name: str) -> Path:
    """Resolve a profile YAML path safely under `profiles_dir`, rejecting traversal."""
    safe_name(name, field="profile")
    path = (profiles_dir / f"{name}.yaml").resolve()
    base = profiles_dir.resolve()
    try:
        path.relative_to(base)
    except ValueError:
        raise UnsafeInputError(f"profile {name!r} escapes profiles dir")
    return path
