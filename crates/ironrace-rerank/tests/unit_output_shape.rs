use ironrace_rerank::output::extract_logits;
use ndarray::{Array, ArrayD, IxDyn};

fn arr(shape: &[usize], data: Vec<f32>) -> ArrayD<f32> {
    Array::from_shape_vec(IxDyn(shape), data).unwrap()
}

#[test]
fn extract_1d_n() {
    let a = arr(&[3], vec![0.1, 0.2, 0.3]);
    assert_eq!(extract_logits(&a).unwrap(), vec![0.1, 0.2, 0.3]);
}

#[test]
fn extract_2d_n_by_1() {
    let a = arr(&[3, 1], vec![0.1, 0.2, 0.3]);
    assert_eq!(extract_logits(&a).unwrap(), vec![0.1, 0.2, 0.3]);
}

#[test]
fn extract_2d_1_by_n() {
    let a = arr(&[1, 3], vec![0.1, 0.2, 0.3]);
    assert_eq!(extract_logits(&a).unwrap(), vec![0.1, 0.2, 0.3]);
}

#[test]
fn extract_rejects_2d_with_multi_columns() {
    let a = arr(&[2, 3], vec![0.0; 6]);
    assert!(extract_logits(&a).is_err());
}
