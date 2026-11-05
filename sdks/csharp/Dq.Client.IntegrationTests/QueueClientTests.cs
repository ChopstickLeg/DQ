using Dq.V1;
using Example.Echo.V1;
using Google.Protobuf;
using Xunit;

namespace Dq.Client.IntegrationTests;

public sealed class QueueClientTests
{
    private const string Service = "example.echo.v1";
    private const string Function = "Reverse";

    [Fact]
    public async Task WorkerDecodesServiceProtobufAndCallerReceivesResult()
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(15));
        var cancellation = timeout.Token;
        await using var server = await RustServer.StartAsync(cancellation);
        using var caller = new QueueClient(server.Address);
        using var worker = new QueueClient(server.Address);

        using var completions = await caller.SubscribeCompletionAsync(Service, [Function], cancellation);
        // The arguments and result fit separately, but exceed 4 MiB together.
        var markerBytes = new byte[3 * 1024 * 1024];
        markerBytes[^1] = 255;
        var marker = ByteString.CopyFrom(markerBytes);
        var arguments = new ReverseRequest { Text = "héllo", Marker = marker };
        var jobId = await caller.EnqueueAsync(
            Service, Function, Guid.NewGuid().ToString(), arguments.ToByteString(),
            caller: "csharp-test", cancellationToken: cancellation);

        using var assignments = worker.SubscribeExecution(Service, [Function], cancellation);
        Assert.True(await assignments.ResponseStream.MoveNext(cancellation));
        var assignment = assignments.ResponseStream.Current;
        Assert.Equal(jobId, assignment.JobId);
        Assert.Equal(Function, assignment.FunctionName);
        var decoded = ReverseRequest.Parser.ParseFrom(assignment.Arguments);
        var result = new ReverseResult { Text = new string(decoded.Text.Reverse().ToArray()), Marker = decoded.Marker };
        await worker.CompleteAsync(assignment, result.ToByteString(), cancellation);

        Assert.True(await completions.ResponseStream.MoveNext(cancellation));
        var completion = completions.ResponseStream.Current;
        Assert.Equal(jobId, completion.JobId);
        Assert.Equal(JobStatus.Completed, completion.Status);
        Assert.Equal(new ReverseResult { Text = "olléh", Marker = marker }, ReverseResult.Parser.ParseFrom(completion.Result));
        var saved = await caller.GetJobStatusAsync(jobId, cancellation);
        Assert.Equal(arguments.ToByteString(), saved.Arguments);
        Assert.Equal(completion.Result, saved.Result);
    }

    [Fact]
    public async Task WorkerErrorReachesCallerAndIsSaved()
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(15));
        var cancellation = timeout.Token;
        await using var server = await RustServer.StartAsync(cancellation);
        using var caller = new QueueClient(server.Address);
        using var worker = new QueueClient(server.Address);

        using var completions = await caller.SubscribeCompletionAsync(Service, [Function], cancellation);
        using var assignments = worker.SubscribeExecution(Service, [Function], cancellation);
        var jobId = await caller.EnqueueAsync(
            Service, Function, Guid.NewGuid().ToString(), ByteString.Empty, cancellationToken: cancellation);
        Assert.True(await assignments.ResponseStream.MoveNext(cancellation));
        await worker.FailAsync(assignments.ResponseStream.Current, "worker could not reverse the input", cancellation);

        Assert.True(await completions.ResponseStream.MoveNext(cancellation));
        var completion = completions.ResponseStream.Current;
        Assert.Equal(jobId, completion.JobId);
        Assert.Equal(JobStatus.Error, completion.Status);
        Assert.Equal("worker could not reverse the input", completion.Error);
        Assert.False(completion.HasResult);
        Assert.Equal(completion.Error, (await caller.GetJobStatusAsync(jobId, cancellation)).Error);
    }
}
