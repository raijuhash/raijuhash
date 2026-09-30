resource "aws_security_group" "ssh" {
  name        = "${var.name}-ssh"
  description = "SSH from the operator public IP"
  vpc_id      = data.aws_vpc.default.id
}

resource "aws_vpc_security_group_ingress_rule" "ssh" {
  security_group_id = aws_security_group.ssh.id
  description       = "SSH"
  ip_protocol       = "tcp"
  from_port         = 22
  to_port           = 22
  cidr_ipv4         = "${chomp(data.http.my_ip.response_body)}/32"
}

resource "aws_vpc_security_group_egress_rule" "all" {
  security_group_id = aws_security_group.ssh.id
  ip_protocol       = "-1"
  cidr_ipv4         = "0.0.0.0/0"
}
