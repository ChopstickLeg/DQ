import os
from pathlib import Path
from queue import Empty, Queue
import subprocess
from tempfile import TemporaryDirectory
from threading import Thread
import unittest
from uuid import uuid4

from dq_python import JobStatus, QueueClient
from echo_pb2 import ReverseRequest, ReverseResult


SERVICE = "example.echo.v1"
FUNCTION = "Reverse"
TIMEOUT = 10.0


class QueueIntegrationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        repo = Path(__file__).resolve().parents[4]
        executable = "dq.exe" if os.name == "nt" else "dq"
        default_binary = repo / "target" / "debug" / executable
        binary = Path(os.environ.get("DQ_SERVER_BIN", str(default_binary)))
        if not binary.is_file():
            raise FileNotFoundError(
                f"Rust server not found at {binary}. Run cargo build --locked or set DQ_SERVER_BIN."
            )

        data_dir = cls.enterClassContext(TemporaryDirectory())
        server = subprocess.Popen(
            [str(binary.resolve())],
            cwd=data_dir,
            env={
                **os.environ,
                "DATABASE_URL": "sqlite://dq.db",
                "LISTEN_ADDR": "127.0.0.1:0",
                "OTEL_SDK_DISABLED": "true",
            },
            stdout=subprocess.PIPE,
            text=True,
        )
        stdout = server.stdout
        assert stdout is not None  # Popen was given stdout=PIPE.
        cls.addClassCleanup(stdout.close)
        cls.addClassCleanup(cls.stop_server, server)

        lines: Queue[str] = Queue()
        Thread(target=lambda: lines.put(stdout.readline()), daemon=True).start()
        try:
            line = lines.get(timeout=TIMEOUT).strip()
        except Empty as error:
            raise RuntimeError("Rust server did not report its listening address") from error
        prefix = "Listening on "
        if not line.startswith(prefix):
            raise RuntimeError(f"Unexpected Rust server startup output: {line!r}")
        cls.address = line.removeprefix(prefix)

    @staticmethod
    def stop_server(server: subprocess.Popen[str]) -> None:
        if server.poll() is None:
            server.kill()
        server.wait(timeout=TIMEOUT)

    def test_worker_decodes_service_protobuf_and_caller_receives_result(self) -> None:
        with QueueClient(self.address) as caller, QueueClient(self.address) as worker:
            completions = caller.subscribe_completion(SERVICE, [FUNCTION], timeout=TIMEOUT)
            arguments = ReverseRequest(text="héllo", marker=b"\x00\xff").SerializeToString()
            job_id = caller.enqueue(SERVICE, FUNCTION, str(uuid4()), arguments, caller="python-test")

            assignments = worker.subscribe_execution(SERVICE, [FUNCTION], timeout=TIMEOUT)
            assignment = next(assignments)
            self.assertEqual(assignment.job_id, job_id)
            self.assertEqual(assignment.function_name, FUNCTION)
            decoded = ReverseRequest.FromString(assignment.arguments)
            result = ReverseResult(text=decoded.text[::-1], marker=decoded.marker)
            worker.complete(assignment, result=result.SerializeToString())

            event = next(completions)
            self.assertEqual(event.job_id, job_id)
            self.assertEqual(event.status, JobStatus.JOB_STATUS_COMPLETED)
            self.assertEqual(
                ReverseResult.FromString(event.result),
                ReverseResult(text="olléh", marker=b"\x00\xff"),
            )
            self.assertEqual(caller.get_job_status(job_id).result, event.result)

    def test_worker_error_reaches_the_caller(self) -> None:
        with QueueClient(self.address) as caller, QueueClient(self.address) as worker:
            completions = caller.subscribe_completion(SERVICE, [FUNCTION], timeout=TIMEOUT)
            assignments = worker.subscribe_execution(SERVICE, [FUNCTION], timeout=TIMEOUT)
            job_id = caller.enqueue(SERVICE, FUNCTION, str(uuid4()), b"", caller="python-test")
            worker.complete(next(assignments), error="worker could not reverse the input")

            event = next(completions)
            self.assertEqual(event.job_id, job_id)
            self.assertEqual(event.status, JobStatus.JOB_STATUS_ERROR)
            self.assertEqual(event.error, "worker could not reverse the input")
            self.assertFalse(event.HasField("result"))
            self.assertEqual(caller.get_job_status(job_id).error, event.error)
