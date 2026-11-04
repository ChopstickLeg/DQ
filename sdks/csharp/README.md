# DQ C# SDK

`Dq.Client` is an async .NET 10 client for the Rust DQ server. It passes
arguments and results as `ByteString` values. Each application service defines
and generates its own Protobuf messages for those bytes.

Add a project reference to `Dq.Client/Dq.Client.csproj`. The project generates
the queue bindings from `proto/dq.proto` at build time.

## Run the integration tests

Activate a Python 3.13 virtual environment, then run these commands from the
repository root. The cross-language test launches the installed Python SDK as a
separate worker process. Set `DQ_PYTHON_BIN` to select a Python executable that
has the SDK installed, or leave it unset to use `python` from `PATH`.

See the [server build instructions](../../README.md#build-and-run-the-service)
for the compile-time SQLite setup.

```powershell
cargo build --locked
python -m pip install -e sdks/python/dq_python
dotnet test sdks/csharp/Dq.Client.IntegrationTests
```

Each test starts the compiled server on an available local port with a temporary
database. Set `DQ_SERVER_BIN` to use a binary outside `target/debug`.

The cross-language test submits one fake voice job before starting the Python
worker and another after it registers. It verifies the saved artifact reference,
job and attempt IDs, and that a duplicate completion report produces one event.
The fixture schema is `Dq.Client.IntegrationTests/Fixtures/voice.proto`. To
regenerate `voice_pb2.py` after a schema edit, install `grpcio-tools==1.78.0`
and run the following command from that fixture directory.

```text
python -m grpc_tools.protoc -I. --python_out=. voice.proto
```

## Call a function in another client

In the worker process, subscribe to the service and functions the worker can
execute. `ReverseRequest` and `ReverseResult` are generated from the
[example service proto](../python/dq_python/tests/echo.proto).

```csharp
using Dq.Client;
using Example.Echo.V1;
using Google.Protobuf;
using Grpc.Core;

using var worker = new QueueClient();
using var assignments = worker.SubscribeExecution("example.echo.v1", ["Reverse"]);
await foreach (var job in assignments.ResponseStream.ReadAllAsync())
{
    var input = ReverseRequest.Parser.ParseFrom(job.Arguments);
    var result = new ReverseResult { Text = new string(input.Text.Reverse().ToArray()), Marker = input.Marker };
    await worker.CompleteAsync(job, result.ToByteString());
}
```

In the caller process, await the completion subscription before submitting the
job. The returned job ID identifies the event for that submission.

```csharp
using Dq.Client;
using Dq.V1;
using Example.Echo.V1;
using Google.Protobuf;
using Grpc.Core;

using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(10));
using var caller = new QueueClient();
using var completions = await caller.SubscribeCompletionAsync("example.echo.v1", ["Reverse"], timeout.Token);
var input = new ReverseRequest { Text = "hello" };
var jobId = await caller.EnqueueAsync(
    "example.echo.v1", "Reverse", Guid.NewGuid().ToString(), input.ToByteString(),
    cancellationToken: timeout.Token);
await foreach (var completion in completions.ResponseStream.ReadAllAsync(timeout.Token))
{
    if (completion.JobId != jobId)
    {
        continue;
    }

    Console.WriteLine(completion.Status == JobStatus.Completed
        ? ReverseResult.Parser.ParseFrom(completion.Result).Text
        : completion.Error);
    break;
}
```

Workers report errors with `FailAsync(job, error)`. Call
`GetJobStatusAsync(jobId)` to retrieve a saved outcome after missing a live
event. `QueueClient` defaults to plaintext gRPC at `http://127.0.0.1:50051`.
