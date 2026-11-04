# DQ Python SDK

This synchronous SDK connects to a running Rust DQ server. Callers submit jobs,
workers execute them, and completion subscribers receive the outcomes. Each
application service defines its own Protobuf messages for arguments and results.
DQ carries those messages as bytes.

## Run the integration tests

Build the server from the repository root with `cargo build --locked`. Follow the
[server build instructions](../../../README.md#build-and-run-the-service) for
the compile-time SQLite database setup.

From `sdks/python/dq_python`, create and activate a Python 3.13 virtual environment.
Then install the SDK and run the tests.

```text
python -m pip install -e .
python -m unittest discover -s tests -v
```

The tests start the compiled Rust binary on a free local port with a temporary
database. Set `DQ_SERVER_BIN` to use a binary outside `target/debug`.

## Call a function in another client

In the worker process, subscribe to the functions the worker can execute.
`ReverseRequest` and `ReverseResult` come from the service's own `echo.proto`.

```python
from dq_python import QueueClient
from echo_pb2 import ReverseRequest, ReverseResult

with QueueClient() as worker:
    for job in worker.subscribe_execution("example.echo.v1", ["Reverse"]):
        args = ReverseRequest.FromString(job.arguments)
        result = ReverseResult(text=args.text[::-1], marker=args.marker)
        worker.complete(job, result=result.SerializeToString())
```

In the caller process, subscribe before submitting to receive the live event.
Use the returned job ID to pick out your job's outcome.

```python
from uuid import uuid4

from dq_python import JobStatus, QueueClient
from echo_pb2 import ReverseRequest, ReverseResult

with QueueClient() as caller:
    events = caller.subscribe_completion("example.echo.v1", ["Reverse"], timeout=10)
    args = ReverseRequest(text="hello")
    job_id = caller.enqueue("example.echo.v1", "Reverse", str(uuid4()), args.SerializeToString())
    for event in events:
        if event.job_id == job_id:
            if event.status == JobStatus.JOB_STATUS_COMPLETED:
                print(ReverseResult.FromString(event.result).text)
            else:
                print(event.error)
            break
```

Workers can also report failures with `worker.complete(job, error="reason")`.
Call `get_job_status(job_id)` to retrieve a saved outcome after missing a live
event. The status response includes both arguments and the outcome and is subject
to the [gRPC message size limit](../../../docs/protocol.md#grpc-message-size-limits).
Pass `host:port` to `QueueClient` to connect to another address. The SDK
uses plaintext gRPC for local testing.

## Regenerate the bindings

Run these commands from the SDK directory after changing the queue protocol or
the example service schema. The generated bindings are included in the SDK.

```text
python -m pip install grpcio-tools==1.78.0
python -m grpc_tools.protoc -Idq_python/_generated=../../../proto --python_out=src --grpc_python_out=src ../../../proto/dq.proto
python -m grpc_tools.protoc -Itests --python_out=tests tests/echo.proto
```

The example bindings are generated into `tests`. Run the examples with that
directory on `PYTHONPATH`, or generate your own service bindings where your
application can import them.
