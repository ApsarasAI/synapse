import inspect
import json
import re
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, AsyncIterator, Dict, Mapping, Optional
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen
from urllib.parse import urlparse, urlunparse

if TYPE_CHECKING:
    import httpx

class SynapseAPIError(RuntimeError):
    def __init__(self, code: str, message: str, status_code: int | None = None):
        super().__init__(message)
        self.code = code
        self.message = message
        self.status_code = status_code


class AuthRequiredError(SynapseAPIError):
    pass


class AuthInvalidError(SynapseAPIError):
    pass


class TenantForbiddenError(SynapseAPIError):
    pass


ERROR_TYPES = {
    "auth_required": AuthRequiredError,
    "auth_invalid": AuthInvalidError,
    "tenant_forbidden": TenantForbiddenError,
}


@dataclass(slots=True)
class SynapseClientConfig:
    base_url: str
    token: Optional[str] = None
    tenant_id: Optional[str] = None
    timeout: float = 30.0


class SynapseClient:
    def __init__(self, config: SynapseClientConfig):
        self._config = config

    def execute(
        self,
        code: str,
        *,
        language: str = "python",
        timeout_ms: int = 5_000,
        memory_limit_mb: int = 128,
        cpu_time_limit_ms: int | None = None,
        runtime_version: str | None = None,
        request_id: str | None = None,
        tenant_id: str | None = None,
    ) -> Dict[str, Any]:
        payload = self._request_payload(
            code=code,
            language=language,
            timeout_ms=timeout_ms,
            memory_limit_mb=memory_limit_mb,
            cpu_time_limit_ms=cpu_time_limit_ms,
            runtime_version=runtime_version,
            request_id=request_id,
            tenant_id=tenant_id,
        )
        response = _post_json(
            f"{self._config.base_url.rstrip('/')}/execute",
            payload,
            self._headers(tenant_id or self._config.tenant_id),
            self._config.timeout,
        )
        return self._decode_response(response)

    async def execute_stream(
        self,
        code: str,
        *,
        language: str = "python",
        timeout_ms: int = 5_000,
        memory_limit_mb: int = 128,
        cpu_time_limit_ms: int | None = None,
        runtime_version: str | None = None,
        request_id: str | None = None,
        tenant_id: str | None = None,
    ) -> AsyncIterator[Dict[str, Any]]:
        payload = self._request_payload(
            code=code,
            language=language,
            timeout_ms=timeout_ms,
            memory_limit_mb=memory_limit_mb,
            cpu_time_limit_ms=cpu_time_limit_ms,
            runtime_version=runtime_version,
            request_id=request_id,
            tenant_id=tenant_id,
        )
        websockets = _import_websockets()
        ws_url = _http_to_ws_url(f"{self._config.base_url.rstrip('/')}/execute/stream")
        async with websockets.connect(ws_url, **_websocket_connect_kwargs(
            websockets,
            self._headers(tenant_id or self._config.tenant_id),
            self._config.timeout,
        )) as websocket:
            await websocket.send(json.dumps(payload))
            async for message in websocket:
                event = json.loads(message)
                if event.get("event") == "error":
                    fields = event.get("fields", {})
                    raise _error_from_code(
                        _normalize_error_code(fields.get("error_code", "execution_failed")),
                        fields.get("error", "stream execution failed"),
                    )
                yield event

    def _request_payload(
        self,
        *,
        code: str,
        language: str,
        timeout_ms: int,
        memory_limit_mb: int,
        cpu_time_limit_ms: int | None,
        runtime_version: str | None,
        request_id: str | None,
        tenant_id: str | None,
    ) -> Dict[str, Any]:
        payload: Dict[str, Any] = {
            "language": language,
            "code": code,
            "timeout_ms": timeout_ms,
            "memory_limit_mb": memory_limit_mb,
        }
        if cpu_time_limit_ms is not None:
            payload["cpu_time_limit_ms"] = cpu_time_limit_ms
        if runtime_version is not None:
            payload["runtime_version"] = runtime_version
        if request_id is not None:
            payload["request_id"] = request_id
        effective_tenant = tenant_id or self._config.tenant_id
        if effective_tenant is not None:
            payload["tenant_id"] = effective_tenant
        return payload

    def _headers(self, tenant_id: str | None) -> Mapping[str, str]:
        headers: Dict[str, str] = {"content-type": "application/json"}
        if self._config.token:
            headers["authorization"] = f"Bearer {self._config.token}"
        if tenant_id:
            headers["x-synapse-tenant-id"] = tenant_id
        return headers

    def _decode_response(self, response: "httpx.Response") -> Dict[str, Any]:
        data = response.json()
        error = data.get("error")
        if error:
            raise _error_from_code(
                error.get("code", "execution_failed"),
                error.get("message") or data.get("stderr", "request failed"),
                response.status_code,
            )
        return data


def _error_from_code(
    code: str,
    message: str,
    status_code: int | None = None,
) -> SynapseAPIError:
    error_type = ERROR_TYPES.get(code, SynapseAPIError)
    return error_type(code=code, message=message, status_code=status_code)


def _import_httpx():
    try:
        import httpx
    except ModuleNotFoundError as exc:
        return None
    return httpx


def _post_json(
    url: str,
    payload: Mapping[str, Any],
    headers: Mapping[str, str],
    timeout: float,
):
    httpx = _import_httpx()
    if httpx is not None:
        with httpx.Client(timeout=timeout) as client:
            return client.post(url, json=payload, headers=headers)
    return _stdlib_post_json(url, payload, headers, timeout)


@dataclass
class _StdlibResponse:
    status_code: int
    body: str

    def json(self) -> Dict[str, Any]:
        return json.loads(self.body)


def _stdlib_post_json(
    url: str,
    payload: Mapping[str, Any],
    headers: Mapping[str, str],
    timeout: float,
) -> _StdlibResponse:
    request = Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers=dict(headers),
        method="POST",
    )
    try:
        with urlopen(request, timeout=timeout) as response:
            return _StdlibResponse(
                status_code=getattr(response, "status", response.getcode()),
                body=response.read().decode("utf-8"),
            )
    except HTTPError as error:
        return _StdlibResponse(
            status_code=error.code,
            body=error.read().decode("utf-8"),
        )
    except URLError as exc:
        raise RuntimeError(f"failed to connect to Synapse API: {exc}") from exc


def _import_websockets():
    try:
        import websockets
    except ModuleNotFoundError as exc:
        raise RuntimeError(
            "websockets is required for SynapseClient.execute_stream(); install sdk/python dependencies first"
        ) from exc
    return websockets


def _websocket_connect_kwargs(
    websockets: Any,
    headers: Mapping[str, str],
    timeout: float,
) -> Dict[str, Any]:
    kwargs: Dict[str, Any] = {"open_timeout": timeout}
    parameters = inspect.signature(websockets.connect).parameters
    if "additional_headers" in parameters:
        kwargs["additional_headers"] = headers
    else:
        kwargs["extra_headers"] = headers
    return kwargs


def _normalize_error_code(code: str) -> str:
    normalized = re.sub(r"(?<!^)(?=[A-Z])", "_", code).replace("-", "_")
    return normalized.strip().lower()


def _http_to_ws_url(url: str) -> str:
    parsed = urlparse(url)
    scheme = "wss" if parsed.scheme == "https" else "ws"
    return urlunparse(parsed._replace(scheme=scheme))
