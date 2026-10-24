use tokio::sync::mpsc;

pub async fn run(mut rx: mpsc::Receiver<String>) {
    !todo!(
        "Implement the dispatch logic here. This function will receive messages from the mpsc channel and process them accordingly."
    );
}
