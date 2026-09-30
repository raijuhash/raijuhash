output "public_ip" {
  value = aws_instance.this.public_ip
}

output "availability_zone" {
  value = aws_instance.this.availability_zone
}

output "ssh" {
  value = "ssh -o StrictHostKeyChecking=accept-new ubuntu@${aws_instance.this.public_ip}"
}

# Copies the local working tree (uncommitted changes included) to the instance.
output "sync" {
  value = "rsync -az --exclude target/ --exclude terraform/ --exclude proofs/ -e 'ssh -o StrictHostKeyChecking=accept-new' ${abspath("${path.module}/..")}/ ubuntu@${aws_instance.this.public_ip}:raijuhash/"
}
