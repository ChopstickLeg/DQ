import json
import sys

from dq_python import QueueClient
from voice_pb2 import SynthesizeRequest, SynthesizeResult


def synthesize(request: SynthesizeRequest) -> SynthesizeResult:
    name = "-".join(request.text.lower().split())
    return SynthesizeResult(artifact_reference=f"fixture://voice/{request.voice}/{name}.wav")


def main(address: str) -> None:
    with QueueClient(address) as worker:
        assignments = worker.subscribe_execution("example.voice.v1", ["Synthesize"], timeout=15)
        for index in range(2):
            assignment = next(assignments)
            request = SynthesizeRequest.FromString(assignment.arguments)
            result = synthesize(request).SerializeToString()
            worker.complete(assignment, result=result)
            if index == 0:
                worker.complete(assignment, result=result)
            receipt = {"job_id": assignment.job_id, "attempt_id": assignment.attempt_id}
            print(json.dumps(receipt), flush=True)


if __name__ == "__main__":
    main(sys.argv[1])
