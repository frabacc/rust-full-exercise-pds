/*
    Suggerimenti:
        - Per scrivere un errore fai come nel punto 1.
        - I tipi generici se devono essere passati da un thread come valore devono implementare i tratti Send e Debug! come in 2.
        - Quando il professore dice senza aspettare cicli di CPU, intende una Condvar, come nel punto 3.
        - Le HashMap richiedono i tratti Eq + Hash, BTreeMap e BinaryHeap invece Ord dato che sono ordinate (BH ha il più grande on top)
        - Quando dobbiamo passare funzioni che implementano tratti funzionali e un tipo generico si fa come il punto 4.
        - Nel punto 4. abbiamo anche una funzione che testa se panica, grazie alla std::panic::panic_unwind
        - Possiamo inserire con una cv su un lock in questo modo 6.
        - Per definire degli interi usiamo gli usize
*/


use core::num;
use std::cell::{Cell, RefCell};
use std::collections::BinaryHeap;
use std::fs::read;
use std::io::empty;
use std::process::Command;
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};
use std::cmp::Ordering;
use std::panic::{self, UnwindSafe};
use std::fmt::Debug;
use std::sync::{mpsc, Arc, Condvar, Mutex, RwLock};
use std::ops::Deref;

use rand::Rng;



/// 1. Errore scritto velocemente, senza campi.
#[derive(Debug)]
struct MyError;

impl MyError{
    fn new() -> Self { return MyError{}}
}

/// 2. Implementazione tratti Send e Debug per struct Item, per essere inviati tra thread come
/// 
#[derive(Debug)]
struct Item<T: Send> {
    t: T,
    i: Instant,
    r: f64,
}

// Ricordati che Eq viene ricavata da PartialEq e che Eq è un sottotratto di Ord
impl<T: Send> Ord for Item<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        other.i.cmp(&self.i)
    }
}

impl<T: Send> PartialOrd for Item<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(other.i.cmp(&self.i))
    }
}

impl<T: Send> PartialEq for Item<T> {
    fn eq(&self, other: &Self) -> bool {
        self.i == other.i
    }
}

impl<T: Send> Eq for Item<T> {}



/// 3. Struttura condivisa usata per le attese condizionate
#[derive(Debug)]
struct MyStruct<T:Send + Ord> {
    struttura_condivisa: Mutex<BinaryHeap<T>>,
    cv: Condvar
}

/// 6. Item da cui implementare Deref
struct Cerchio {
    r: f64,
}

impl Deref for Cerchio {
    type Target = f64;
    fn deref(&self) -> &Self::Target {
        &self.r
    }
}



///4. Implementazione della struttura condivisa
impl<T:Send + Ord> MyStruct<T> {
    fn new() -> Self {
        MyStruct { struttura_condivisa: Mutex::new(BinaryHeap::<T>::new()), cv: Condvar::new() }
    }

    // Come puoi notare, non è necessario specificare il tipo generico, ma essa deve implementare UnwindSafe e restituire un risultato!
    fn test_panic(f: impl FnOnce() -> T + UnwindSafe + Send + Debug, t: T) -> Result<T,MyError> {
        
        // Test panic della funzione passata
        let result = std::panic::catch_unwind(f);

        if result.is_err() {
            return Err(MyError);
        }
        else {
            return Ok(t);
        }
    }

    // 
    fn insert(&self ,t: T) -> Result<(),MyError> {
        let mut lock = self.struttura_condivisa.lock().unwrap();

        // Questa struttura serve ad aspettare per inserire le cose quando è vuota
        lock = self.cv.wait_while(lock, |lock| {
            lock.len() == 4
        }).unwrap();
        let initial_length = lock.len();
        lock.push(t);
        let final_length = lock.len();

        if initial_length == final_length + 1 {
            
                self.cv.notify_all();
                return Ok(());
        } else {
            self.cv.notify_all();
            Err(MyError)
        }
    }

    fn extract(&self) -> Result<T, MyError> {
        let mut lock = self.struttura_condivisa.lock().unwrap();
        lock = self.cv.wait_while(lock, |lock| {
            lock.len() == 0 //vorrei provare con questo semplicemente != 4 (va in deadlock)
        }).unwrap();
        let popped_item = lock.pop();
        
        match popped_item {
            Some(p) => {
                self.cv.notify_all();
                return Ok(p);
            }
            None => Err(MyError)
        }
    }
}
 

fn cv_example() {
    let bh = Arc::new(MyStruct::new());

    let mut rng = rand::thread_rng();
    let random_number = rng.gen_range(0..10);

    let mut handles = vec![];

    for i in 0..10 {
        let bh = bh.clone();
        handles.push(thread::spawn(move|| {
            let result = bh.insert(i);
            if result.is_ok() {
                println!("Inserted {i} in BinaryHeap");
                println!("{:?}",bh.struttura_condivisa.lock());
            } else {
                println!("Error inserting value");
                println!("{:?}",bh);
            }
        }))
    }

    for i in 0..10 {
        let bh = bh.clone();
        handles.push(thread::spawn(move || {
            let popped_result = bh.extract();
            if popped_result.is_ok() {
                println!("{:?}: questo è il risultato uscito dalla coda", popped_result);
            }
        }))
    }

    for t in handles { t.join().unwrap();}
}


/// Inizio parte dei messaggi, iniziamo con una struttura che invia messaggi, deve essere associata ad un ricevitore
struct MySenderStruct<T: Send> {
    sender: Sender<T>,
    receiver: MyReceiverStruct<T> //se sincrono può essere SyncSender
}

struct MyReceiverStruct<T: Send> {
    receiver: Receiver<T>,
    coda_messaggi: Vec<T>
}

impl<T: Send + Debug> MySenderStruct<T> {
    fn new() -> Self {
        // Attenzionare come si creano delle strutture di messaggi
        // Quando usiamo i messaggi non c'è necessità di mutex!!!
        let (tx,rx) = channel();

        MySenderStruct {
            sender: tx,
            receiver : MyReceiverStruct{
                receiver: rx,
                coda_messaggi: Vec::<T>::new(),
            }
        }
    }

    fn send(&self, msg: T) -> Result<(), MyError> {
        println!("Sending: {:?} ...",msg);
        let result = self.sender.send(msg);
        if result.is_ok() {
            println!("Sent.");
            Ok(())
        } else {
            Err(MyError)
        }
    }

    fn recv(&self) -> Result<T,MyError> {
        let msg = self.receiver.receiver.recv().unwrap();
        return Ok(msg);
    }


}

fn tx_rx_rendez_vous_example() {
    let (tx,rx) = sync_channel(0); // Per avere messaggi ricevuti in ordine di invio!!
    let mut handles = vec![];


    for i in 0..4 {
        let tx_clone = tx.clone(); //questo se fosse fuori non sarebbe necessario il drop
        let handle = thread::spawn(move || {
            println!("sending ciao melo {}",i);
            let _ = tx_clone.send(format!("ciao melo {}",i)).unwrap(); // Questa send resituisce o Ok(()) o SendError<T>
        });
        handles.push(handle);

    }
    drop(tx); // Il drop è necessario... solo perchè cloniamo all'interno del ciclo....

    for rx in rx {
        println!("Receiver said : {}",rx);
    }

    //Come vedi i messaggi sono ricevuti in ordine di invio!!!
}

fn duration_and_time() {
    
    let mut counter = 0;
    let time_limit = Duration::from_secs(1);
    let start = Instant::now();

    while (Instant::now() - start) < time_limit {
        counter+=1;
    }

    println!("{}",counter);


}

fn cv_simple() {
    let shared_structure = Arc::new((Mutex::new(5), Condvar::new()));
    let cloned_structure = Arc::clone(&shared_structure);
    let second_cloned_structure = Arc::clone(&shared_structure);

    //puoi fare la clonazione all'interno di un ciclo for, per ora va bene così

    thread::spawn(move || {
        let (lock, cv) = &*cloned_structure;
        let mut number_to_modify = lock.lock().unwrap();

        *number_to_modify = 4;
        println!("Changing the number to 4 so it still doesnt pass the wait_while condition.");
        cv.notify_one();
    });

    thread::spawn(move || {
        let (lock, cv) = &*second_cloned_structure;
        let mut number_to_modify = lock.lock().unwrap();

        *number_to_modify = 12;
        println!("Changing the number to 12 so it still doesnt pass the wait_while condition.");
        cv.notify_one();
    });

    let (lock,cv) = &*shared_structure;
    let mut number_to_check = lock.lock().unwrap();
    println!("checking if condition passes...");
    // La variabile number to check può essere rinominata in qualsiasi modo nella condition! è quello che il lock protegge come
    // parametro (se c'è una tupla, la tupla altrimenti una struct o qualsiasi altra variabile protetta)
    number_to_check = cv.wait_while(number_to_check, |suca| {*suca < 10}).unwrap();
    println!("condition passed!!!");

    
}




//in teoria se modificato da più thread Arc, ma noi lo facciamo dopo!
struct MyReadWriteStructure {
    structure: RwLock<Vec<String>>,
    cv: Condvar,
}

impl MyReadWriteStructure {
    
    fn new() -> Self {
        MyReadWriteStructure {
            structure: RwLock::new(vec![]),
            cv: Condvar::new()
        }
    }

    fn read_full_vec(&self, thread_id: usize) {

        println!("Eseguito thread {} lettore",thread_id);

        let read_lock = self.structure.read().unwrap();

        for (i,msg) in read_lock.iter().enumerate() {
            println!("thread {} ha letto nella posizione {}: {}",thread_id,i,msg);
        }
    }

    fn write_full_vec(&self, thread_id: usize) {

        println!("Eseguito thread {} scrittore",thread_id);
        let mut write_lock = self.structure.write().unwrap();
        *write_lock = vec![];

        for i in 0..thread_id {
            write_lock.push(format!("Ciao, il thread {} ha scritto qui!",thread_id));
        }

    }
}

fn rwlock_example() {
    
    let struttura = Arc::new(MyReadWriteStructure::new());

    let mut read_handles = vec![];
    let mut write_handles = vec![];

    for i in 0..15 {
        let cloned_struttura = Arc::clone(&struttura);
        let mut ref_write = &mut write_handles;
        ref_write.push(thread::spawn(move || {
            cloned_struttura.write_full_vec(i);
        }));

    }

    for i in 0..10 {
        let cloned_struttura = Arc::clone(&struttura);
        let mut ref_read = &mut read_handles;
        ref_read.push(thread::spawn(move || {
            cloned_struttura.read_full_vec(i);
        }));
    }

    println!("Eseguito thread principale!");

    for t in write_handles {t.join().unwrap();}
    for t in read_handles {t.join().unwrap();}
    
}



fn main() {
    //cv_example();
    //tx_rx_rendez_vous_example();
    //duration_and_time();
    //cv_simple();
    //rwlock_example();

    let c = Cerchio{r: 12.0};

    println!("{:p}",c.deref());
}
