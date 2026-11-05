using System.Diagnostics;
using System.Text.Json;
using System.Text.Json.Serialization;
using Dq.V1;
using Example.Voice.V1;
using Google.Protobuf;
using Xunit;

namespace Dq.Client.IntegrationTests;

public sealed class PythonWorkerTests
{
    private const string Service = "example.voice.v1";
    private const string Function = "Synthesize";

    [Fact]
    public async Task CSharpCallerReceivesPersistedVoiceResultFromPythonWorker()
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(20));
        var cancellation = timeout.Token;
        await using var server = await RustServer.StartAsync(cancellation);
        using var caller = new QueueClient(server.Address);
        using var completions = await caller.SubscribeCompletionAsync(Service, [Function], cancellation);

        var offlineJob = await EnqueueAsync(caller, "Hello Queue", cancellation);
        var pending = await caller.GetJobStatusAsync(offlineJob, cancellation);
        Assert.Equal(JobStatus.Pending, pending.Status);
        Assert.False(pending.HasResult);

        using var worker = StartWorker(server.Address);
        try
        {
            var first = await ReadReceiptAsync(worker, cancellation);
            Assert.Equal(offlineJob, first.JobId);
            Assert.True(Guid.TryParse(first.AttemptId, out _));
            Assert.True(await completions.ResponseStream.MoveNext(cancellation));
            var completion = completions.ResponseStream.Current;
            Assert.Equal(offlineJob, completion.JobId);
            Assert.Equal(JobStatus.Completed, completion.Status);

            var saved = await caller.GetJobStatusAsync(offlineJob, cancellation);
            Assert.Equal(JobStatus.Completed, saved.Status);
            Assert.True(saved.HasResult);
            Assert.Equal(completion.Result, saved.Result);
            Assert.Equal("fixture://voice/narrator/hello-queue.wav",
                SynthesizeResult.Parser.ParseFrom(saved.Result).ArtifactReference);

            // The next job's event acts as a barrier. A replay of the first completion would arrive first.
            var onlineJob = await EnqueueAsync(caller, "Second Job", cancellation);
            var second = await ReadReceiptAsync(worker, cancellation);
            Assert.Equal(onlineJob, second.JobId);
            Assert.True(Guid.TryParse(second.AttemptId, out _));
            Assert.NotEqual(first.AttemptId, second.AttemptId);
            Assert.True(await completions.ResponseStream.MoveNext(cancellation));
            Assert.Equal(onlineJob, completions.ResponseStream.Current.JobId);
            await worker.WaitForExitAsync(cancellation);
            Assert.Equal(0, worker.ExitCode);
        }
        finally
        {
            if (!worker.HasExited)
            {
                try
                {
                    worker.Kill();
                }
                catch (InvalidOperationException) when (worker.HasExited)
                {
                    // The worker exited before the kill request.
                }
                using var cleanupTimeout = new CancellationTokenSource(TimeSpan.FromSeconds(5));
                await worker.WaitForExitAsync(cleanupTimeout.Token);
            }
        }
    }

    private static Task<string> EnqueueAsync(QueueClient caller, string text, CancellationToken cancellationToken)
    {
        var request = new SynthesizeRequest { Text = text, Voice = "narrator" };
        return caller.EnqueueAsync(Service, Function, Guid.NewGuid().ToString(), request.ToByteString(),
            cancellationToken: cancellationToken);
    }

    private static Process StartWorker(string address)
    {
        var start = new ProcessStartInfo
        {
            FileName = Environment.GetEnvironmentVariable("DQ_PYTHON_BIN") ?? "python",
            UseShellExecute = false,
            RedirectStandardOutput = true,
        };
        start.ArgumentList.Add(Path.Combine(AppContext.BaseDirectory, "Fixtures", "voice_worker.py"));
        start.ArgumentList.Add(new Uri(address).Authority);
        return Process.Start(start) ?? throw new InvalidOperationException("Could not start the Python worker.");
    }

    private static async Task<WorkerReceipt> ReadReceiptAsync(Process worker, CancellationToken cancellationToken)
    {
        var line = await worker.StandardOutput.ReadLineAsync(cancellationToken)
            ?? throw new InvalidOperationException("Python worker exited without reporting a result. Check that dq_python is installed.");
        return JsonSerializer.Deserialize<WorkerReceipt>(line)
            ?? throw new InvalidOperationException("Python worker returned an empty receipt.");
    }

    private sealed record WorkerReceipt(
        [property: JsonPropertyName("job_id")] string JobId,
        [property: JsonPropertyName("attempt_id")] string AttemptId);
}
