import asyncio
import json
import pathlib
import sys
import types
import unittest
from unittest import mock

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "src"))

from synapse_sdk.client import (
    SynapseAPIError,
    SynapseClient,
    SynapseClientConfig,
    _websocket_connect_kwargs,
)


class _DummyConnect:
    def __call__(self, *_args, **_kwargs):
        raise AssertionError("connect should not be invoked in this test")


class _FakeWebSocket:
    def __init__(self, messages):
        self._messages = iter(messages)
        self.sent = []

    async def send(self, payload):
        self.sent.append(payload)

    def __aiter__(self):
        return self

    async def __anext__(self):
        try:
            return next(self._messages)
        except StopIteration as exc:
            raise StopAsyncIteration from exc


class _FakeConnectContext:
    def __init__(self, websocket):
        self.websocket = websocket

    async def __aenter__(self):
        return self.websocket

    async def __aexit__(self, exc_type, exc, tb):
        return False


class _FakeWebsocketsModule:
    def __init__(self, websocket):
        self.websocket = websocket
        self.calls = []

    def connect(self, url, **kwargs):
        self.calls.append((url, kwargs))
        return _FakeConnectContext(self.websocket)


class SynapseClientTests(unittest.TestCase):
    def test_websocket_connect_kwargs_supports_additional_headers(self):
        fake_module = types.SimpleNamespace(connect=_DummyConnect())
        with mock.patch("inspect.signature") as signature:
            signature.return_value = types.SimpleNamespace(
                parameters={"additional_headers": object()}
            )
            kwargs = _websocket_connect_kwargs(
                fake_module,
                {"authorization": "Bearer token"},
                3.5,
            )

        self.assertEqual(kwargs["open_timeout"], 3.5)
        self.assertEqual(
            kwargs["additional_headers"],
            {"authorization": "Bearer token"},
        )
        self.assertNotIn("extra_headers", kwargs)

    def test_websocket_connect_kwargs_supports_extra_headers(self):
        fake_module = types.SimpleNamespace(connect=_DummyConnect())
        with mock.patch("inspect.signature") as signature:
            signature.return_value = types.SimpleNamespace(
                parameters={"extra_headers": object()}
            )
            kwargs = _websocket_connect_kwargs(
                fake_module,
                {"authorization": "Bearer token"},
                3.5,
            )

        self.assertEqual(kwargs["open_timeout"], 3.5)
        self.assertEqual(
            kwargs["extra_headers"],
            {"authorization": "Bearer token"},
        )
        self.assertNotIn("additional_headers", kwargs)

    def test_execute_stream_emits_events(self):
        async def run_test():
            websocket = _FakeWebSocket(
                [
                    json.dumps({"event": "started", "fields": {"request_id": "req-1"}}),
                    json.dumps({"event": "completed", "fields": {"exit_code": 0}}),
                ]
            )
            fake_websockets = _FakeWebsocketsModule(websocket)
            client = SynapseClient(SynapseClientConfig(base_url="http://synapse.test"))

            with mock.patch(
                "synapse_sdk.client._import_websockets",
                return_value=fake_websockets,
            ):
                events = []
                async for event in client.execute_stream(
                    "print('ok')\n",
                    request_id="req-1",
                ):
                    events.append(event)

            self.assertEqual([event["event"] for event in events], ["started", "completed"])
            self.assertEqual(len(fake_websockets.calls), 1)
            url, kwargs = fake_websockets.calls[0]
            self.assertEqual(url, "ws://synapse.test/execute/stream")
            self.assertEqual(kwargs["open_timeout"], 30.0)
            self.assertIn("extra_headers", kwargs)
            self.assertEqual(
                json.loads(websocket.sent[0])["request_id"],
                "req-1",
            )

        asyncio.run(run_test())

    def test_execute_stream_maps_error_events(self):
        async def run_test():
            websocket = _FakeWebSocket(
                [
                    json.dumps(
                        {
                            "event": "error",
                            "fields": {
                                "error_code": "tenantForbidden",
                                "error": "tenant rejected",
                            },
                        }
                    )
                ]
            )
            fake_websockets = _FakeWebsocketsModule(websocket)
            client = SynapseClient(SynapseClientConfig(base_url="http://synapse.test"))

            with mock.patch(
                "synapse_sdk.client._import_websockets",
                return_value=fake_websockets,
            ):
                with self.assertRaises(SynapseAPIError) as error:
                    async for _event in client.execute_stream("print('blocked')\n"):
                        pass

            self.assertEqual(error.exception.code, "tenant_forbidden")
            self.assertEqual(error.exception.message, "tenant rejected")

        asyncio.run(run_test())


if __name__ == "__main__":
    unittest.main()
